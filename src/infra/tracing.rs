//! Tracing services.

use std::collections::HashMap;
use std::fmt::Debug;
use std::fmt::Display;
use std::io::stdout;
use std::io::IsTerminal;
use std::net::SocketAddr;
use std::str::FromStr;
use std::thread::Thread;

use anyhow::anyhow;
use chrono::DateTime;
use chrono::Local;
use chrono::Utc;
use console_subscriber::ConsoleLayer;
use display_json::DebugAsJson;
use itertools::Itertools;
use opentelemetry::KeyValue;
use opentelemetry_otlp::Protocol;
use opentelemetry_otlp::SpanExporterBuilder;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::runtime;
use opentelemetry_sdk::trace;
use opentelemetry_sdk::trace::BatchConfigBuilder;
use opentelemetry_sdk::trace::Tracer as SdkTracer;
use opentelemetry_sdk::Resource as SdkResource;
use serde::ser::SerializeStruct;
use serde::Serialize;
use serde_json::Value as JsonValue;
use tonic::metadata::MetadataKey;
use tonic::metadata::MetadataMap;
use tracing::span;
use tracing::span::Attributes;
use tracing::Event;
use tracing::Span;
use tracing::Subscriber;
use tracing_serde::fields::AsMap;
use tracing_serde::fields::SerializeFieldMap;
use tracing_serde::AsSerde;
use tracing_serde::SerializeLevel;
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::format::DefaultFields;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::fmt::FormatEvent;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
use ulid::Ulid;

use crate::config::TracingConfig;
use crate::ext::not;
use crate::ext::spawn_named;
use crate::ext::to_json_string;
use crate::ext::to_json_value;
use crate::infra::build_info;

/// Init application tracing.
pub async fn init_tracing(config: &TracingConfig, sentry_url: Option<&str>, tokio_console_address: SocketAddr) -> anyhow::Result<()> {
    println!("creating tracing registry");

    // configure tracing context layer

    println!("tracing registry: enabling tracing context recorder");
    let tracing_context_layer = TracingContextLayer.with_filter(EnvFilter::from_default_env());

    // configure stdout log layer
    let enable_ansi = stdout().is_terminal();
    println!(
        "tracing registry: enabling console logs | format={} ansi={}",
        config.tracing_log_format, enable_ansi
    );
    let stdout_layer = match config.tracing_log_format {
        TracingLogFormat::Json => fmt::Layer::default()
            .event_format(JsonFormatter)
            .with_filter(EnvFilter::from_default_env())
            .boxed(),
        TracingLogFormat::Minimal => fmt::Layer::default()
            .with_thread_ids(false)
            .with_thread_names(false)
            .with_target(false)
            .with_ansi(enable_ansi)
            .with_timer(MinimalTimer)
            .with_filter(EnvFilter::from_default_env())
            .boxed(),
        TracingLogFormat::Normal => fmt::Layer::default().with_ansi(enable_ansi).with_filter(EnvFilter::from_default_env()).boxed(),
        TracingLogFormat::Verbose => fmt::Layer::default()
            .with_ansi(enable_ansi)
            .with_target(true)
            .with_thread_ids(true)
            .with_thread_names(true)
            .with_filter(EnvFilter::from_default_env())
            .boxed(),
    };

    // configure opentelemetry layer
    let opentelemetry_layer = match &config.tracing_url {
        Some(url) => {
            let tracer = opentelemetry_tracer(url, config.tracing_protocol, &config.tracing_headers);
            let layer = tracing_opentelemetry::layer()
                .with_tracked_inactivity(false)
                .with_tracer(tracer)
                .with_filter(EnvFilter::from_default_env());
            Some(layer)
        }
        None => {
            println!("tracing registry: skipping opentelemetry exporter");
            None
        }
    };

    // configure sentry layer
    let sentry_layer = match sentry_url {
        Some(sentry_url) => {
            println!("tracing registry: enabling sentry exporter | url={}", sentry_url);
            let layer = sentry_tracing::layer().with_filter(EnvFilter::from_default_env());
            Some(layer)
        }
        None => {
            println!("tracing registry: skipping sentry exporter");
            None
        }
    };

    // configure tokio-console layer
    println!("tracing registry: enabling tokio console exporter | address={}", tokio_console_address);
    let (console_layer, console_server) = ConsoleLayer::builder().with_default_env().server_addr(tokio_console_address).build();
    spawn_named("console::grpc-server", async move {
        if let Err(e) = console_server.serve().await {
            tracing::error!(reason = ?e, address = %tokio_console_address, "failed to create tokio-console server");
        };
    });

    // init registry
    let result = tracing_subscriber::registry()
        .with(tracing_context_layer)
        .with(stdout_layer)
        .with(opentelemetry_layer)
        .with(sentry_layer)
        .with(console_layer)
        .try_init();

    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            println!("failed to create tracing registry | reason={:?}", e);
            Err(e.into())
        }
    }
}

fn opentelemetry_tracer(url: &str, protocol: TracingProtocol, headers: &[String]) -> SdkTracer {
    println!(
        "tracing registry: enabling opentelemetry exporter | url={} protocol={} headers={} service={}",
        url,
        protocol,
        headers.len(),
        build_info::service_name()
    );

    // configure headers
    let headers = headers
        .iter()
        .map(|header| {
            let mut parts = header.splitn(2, '=');
            let key = parts.next().unwrap();
            let value = parts.next().unwrap_or_default();
            (key, value)
        })
        .collect_vec();

    // configure tracer
    let tracer_exporter: SpanExporterBuilder = match protocol {
        TracingProtocol::Grpc => {
            let mut protocol_metadata = MetadataMap::new();
            for (key, value) in headers {
                protocol_metadata.insert(MetadataKey::from_str(key).unwrap(), value.parse().unwrap());
            }

            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_protocol(Protocol::Grpc)
                .with_endpoint(url)
                .with_metadata(protocol_metadata)
                .into()
        }
        TracingProtocol::HttpBinary | TracingProtocol::HttpJson => {
            let mut protocol_headers = HashMap::new();
            for (key, value) in headers {
                protocol_headers.insert(key.to_owned(), value.to_owned());
            }

            opentelemetry_otlp::new_exporter()
                .http()
                .with_protocol(protocol.into())
                .with_endpoint(url)
                .with_headers(protocol_headers)
                .into()
        }
    };

    let tracer_config = trace::config().with_resource(SdkResource::new(vec![KeyValue::new("service.name", build_info::service_name())]));

    // configure pipeline
    let batch_config = BatchConfigBuilder::default().with_max_queue_size(u16::MAX as usize).build();
    opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(tracer_exporter)
        .with_trace_config(tracer_config)
        .with_batch_config(batch_config)
        .install_batch(runtime::Tokio)
        .unwrap()
}

// -----------------------------------------------------------------------------
// Tracing config
// -----------------------------------------------------------------------------

/// Tracing event log format.
#[derive(DebugAsJson, strum::Display, Clone, Copy, Eq, PartialEq, serde::Serialize)]
pub enum TracingLogFormat {
    /// Minimal format: Time (no date), level, and message.
    #[serde(rename = "minimal")]
    #[strum(to_string = "minimal")]
    Minimal,

    /// Normal format: Default `tracing` crate configuration.
    #[serde(rename = "normal")]
    #[strum(to_string = "normal")]
    Normal,

    /// Verbose format: Full datetime, level, thread, target, and message.
    #[serde(rename = "verbose")]
    #[strum(to_string = "verbose")]
    Verbose,

    /// JSON format: Verbose information formatted as JSON.
    #[serde(rename = "json")]
    #[strum(to_string = "json")]
    Json,
}

impl FromStr for TracingLogFormat {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        match s.to_lowercase().trim() {
            "json" => Ok(Self::Json),
            "minimal" => Ok(Self::Minimal),
            "normal" => Ok(Self::Normal),
            "verbose" | "full" => Ok(Self::Verbose),
            s => Err(anyhow!("unknown log format: {}", s)),
        }
    }
}

#[derive(DebugAsJson, strum::Display, Clone, Copy, Eq, PartialEq, serde::Serialize)]
pub enum TracingProtocol {
    #[serde(rename = "grpc")]
    #[strum(to_string = "grpc")]
    Grpc,

    #[serde(rename = "http-binary")]
    #[strum(to_string = "http-binary")]
    HttpBinary,

    #[serde(rename = "http-json")]
    #[strum(to_string = "http-json")]
    HttpJson,
}

impl FromStr for TracingProtocol {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        match s.to_lowercase().trim() {
            "grpc" => Ok(Self::Grpc),
            "http-binary" => Ok(Self::HttpBinary),
            "http-json" => Ok(Self::HttpJson),
            s => Err(anyhow!("unknown tracing protocol: {}", s)),
        }
    }
}

impl From<TracingProtocol> for Protocol {
    fn from(value: TracingProtocol) -> Self {
        match value {
            TracingProtocol::Grpc => Self::Grpc,
            TracingProtocol::HttpBinary => Self::HttpBinary,
            TracingProtocol::HttpJson => Self::HttpJson,
        }
    }
}

// -----------------------------------------------------------------------------
// Tracing service: Span field recorder
// -----------------------------------------------------------------------------
struct TracingContextLayer;

impl<S> Layer<S> for TracingContextLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: tracing_subscriber::layer::Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        span.extensions_mut().insert(SpanFields::new(attrs.field_map()));
    }

    fn on_record(&self, id: &span::Id, values: &span::Record<'_>, ctx: tracing_subscriber::layer::Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut extensions = span.extensions_mut();
        if let Some(map) = extensions.get_mut::<SpanFields>() {
            map.record(values.field_map());
        }
    }
}

#[derive(derive_more::Deref, derive_more::DerefMut)]
struct SpanFields(#[deref] JsonValue);

impl SpanFields {
    fn new(fields: SerializeFieldMap<'_, Attributes>) -> Self {
        let fields = to_json_value(fields);
        Self(fields)
    }

    fn record(&mut self, fields: SerializeFieldMap<'_, span::Record>) {
        let mut new_fields = to_json_value(fields);
        let Some(new_fields) = new_fields.as_object_mut() else { return };

        let Some(current_fields) = self.as_object_mut() else { return };
        for (new_field_key, new_field_value) in new_fields.into_iter() {
            current_fields.insert(new_field_key.to_owned(), new_field_value.to_owned());
        }
    }
}

// -----------------------------------------------------------------------------
// Tracing service: - Json Formatter
// -----------------------------------------------------------------------------

struct JsonFormatter;

impl<S> FormatEvent<S, DefaultFields> for JsonFormatter
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn format_event(&self, ctx: &fmt::FmtContext<'_, S, DefaultFields>, mut writer: fmt::format::Writer<'_>, event: &tracing::Event<'_>) -> std::fmt::Result {
        let meta = event.metadata();

        // parse spans
        let context = ctx.lookup_current().map(|span| {
            let mut root_span = None;
            let mut merged_span_context = HashMap::new();

            let mut span_iterator = span.scope().peekable();
            while let Some(span) = span_iterator.next() {
                // merge span data into a single context
                if let Some(span_fields) = span.extensions().get::<SpanFields>().and_then(|fields| fields.as_object()) {
                    for (field_key, field_value) in span_fields {
                        if not(merged_span_context.contains_key(field_key)) {
                            merged_span_context.insert(field_key.to_owned(), field_value.to_owned());
                        }
                    }
                }

                // track root span
                if span_iterator.peek().is_none() {
                    root_span = Some(span);
                }
            }

            // generate context field
            TracingLogContextField {
                root_span_id: root_span.as_ref().map(|s| s.id().into_u64()).unwrap_or(0),
                root_span_name: root_span.as_ref().map(|s| s.name()).unwrap_or(""),
                span_id: span.id().into_u64(),
                span_name: span.name(),
                context: merged_span_context,
            }
        });

        // parse metadata and event
        let log = TracingLog {
            timestamp: Utc::now(),
            level: meta.level().as_serde(),
            target: meta.target(),
            thread: std::thread::current(),
            fields: event.field_map(),
            context,
        };

        writeln!(writer, "{}", to_json_string(&log))
    }
}

#[derive(derive_new::new)]
struct TracingLog<'a> {
    timestamp: DateTime<Utc>,
    level: SerializeLevel<'a>,
    target: &'a str,
    thread: Thread,
    fields: SerializeFieldMap<'a, Event<'a>>,
    context: Option<TracingLogContextField<'a>>,
}

impl<'a> Serialize for TracingLog<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("TracingLog", 7)?;
        state.serialize_field("timestamp", &self.timestamp)?;
        state.serialize_field("level", &self.level)?;
        state.serialize_field("target", self.target)?;
        state.serialize_field("threadId", &format!("{:?}", self.thread.id()))?;
        state.serialize_field("threadName", self.thread.name().unwrap_or_default())?;
        state.serialize_field("fields", &self.fields)?;
        match &self.context {
            Some(context) => state.serialize_field("context", context)?,
            None => state.skip_field("context")?,
        }
        state.end()
    }
}

#[derive(serde::Serialize)]
struct TracingLogContextField<'a> {
    root_span_id: u64,
    root_span_name: &'a str,
    span_id: u64,
    span_name: &'a str,
    #[serde(flatten)]
    context: HashMap<String, JsonValue>,
}

// -----------------------------------------------------------------------------
// Tracing service: - Minimal Timer
// -----------------------------------------------------------------------------
struct MinimalTimer;

impl FormatTime for MinimalTimer {
    fn format_time(&self, w: &mut fmt::format::Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", Local::now().time().format("%H:%M:%S%.3f"))
    }
}

// -----------------------------------------------------------------------------
// Tracing extensions
// -----------------------------------------------------------------------------

/// Extensions for `tracing::Span`.
pub trait SpanExt {
    #[cfg(feature = "tracing")]
    /// Applies the provided function to the current span.
    fn with<F>(fill: F)
    where
        F: Fn(Span),
    {
        let span = Span::current();
        fill(span);
    }

    #[cfg(not(feature = "tracing"))]
    /// Do nothing because the `tracing` function is disabled.
    fn with<F>(_: F)
    where
        F: Fn(Span),
    {
    }

    /// Records a value using `ToString` implementation.
    fn rec_str<T>(&self, field: &'static str, value: &T)
    where
        T: ToString;

    /// Records a value using `ToString` implementation if the option value is present.
    fn rec_opt<T>(&self, field: &'static str, value: &Option<T>)
    where
        T: ToString;
}

impl SpanExt for Span {
    fn rec_str<T>(&self, field: &'static str, value: &T)
    where
        T: ToString,
    {
        self.record(field, value.to_string().as_str());
    }

    fn rec_opt<T>(&self, field: &'static str, value: &Option<T>)
    where
        T: ToString,
    {
        if let Some(ref value) = value {
            self.record(field, value.to_string().as_str());
        }
    }
}

// -----------------------------------------------------------------------------
// TracingExt
// -----------------------------------------------------------------------------

/// Extensions for values used as fields in `tracing` macros.
pub trait TracingExt {
    /// Returns the `Display` value of the inner value or an empty string.
    fn or_empty(&self) -> String;
}

impl<T> TracingExt for Option<T>
where
    T: Display + Debug,
{
    fn or_empty(&self) -> String {
        match self {
            Some(value) => value.to_string(),
            None => String::new(),
        }
    }
}

// -----------------------------------------------------------------------------
// Tracing macros
// -----------------------------------------------------------------------------

/// Logs an error and also wrap the existing error with the provided message.
#[macro_export]
macro_rules! log_and_err {
    // with reason: wrap the original error with provided message
    (reason = $error:ident, payload = $payload:expr, $msg:expr) => {
        {
            use anyhow::Context;
            tracing::error!(reason = ?$error, payload = ?$payload, message = %$msg);
            Err($error).context($msg)
        }
    };
    (reason = $error:ident, $msg:expr) => {
        {
            use anyhow::Context;
            tracing::error!(reason = ?$error, message = %$msg);
            Err($error).context($msg)
        }
    };
    // without reason: generate a new error using provided message
    (payload = $payload:expr, $msg:expr) => {
        {
            use anyhow::Context;
            use anyhow::anyhow;
            tracing::error!(payload = ?$payload, message = %$msg);
            let message = format!("{} | payload={:?}", $msg, $payload);
            Err(anyhow!(message))
        }
    };
    ($msg:expr) => {
        {
            use anyhow::anyhow;
            tracing::error!(message = %$msg);
            Err(anyhow!($msg))
        }
    };
}

// -----------------------------------------------------------------------------
// Tracing functions
// -----------------------------------------------------------------------------

/// Creates a new unique correlation ID.
pub fn new_cid() -> String {
    Ulid::new().to_string()
}

/// Emits an info message that a task was spawned to backgroud.
#[track_caller]
pub fn info_task_spawn(name: &str) {
    tracing::info!(%name, "spawning task");
}

/// Emits an warning that a task is exiting because it received a cancenllation signal.
///
/// Returns the formatted tracing message.
#[track_caller]
pub fn warn_task_cancellation(task: &str) -> String {
    let message = format!("exiting {} because it received a cancellation signal", task);
    tracing::warn!(%message);
    message
}

/// Emits an warning that a task is exiting because the tx side was closed.
///
/// Returns the formatted tracing message.
#[track_caller]
pub fn warn_task_tx_closed(task: &str) -> String {
    let message = format!("exiting {} because the tx channel on the other side was closed", task);
    tracing::warn!(%message);
    message
}

/// Emits an warning that a task is exiting because the rx side was closed.
///
/// Returns the formatted tracing message.
#[track_caller]
pub fn warn_task_rx_closed(task: &str) -> String {
    let message = format!("exiting {} because the rx channel on the other side was closed", task);
    tracing::warn!(%message);
    message
}
