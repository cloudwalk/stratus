//! Tracing services.

use std::collections::HashMap;
use std::fmt::Debug;
use std::fmt::Display;
use std::thread::Thread;

use chrono::DateTime;
use chrono::Local;
use chrono::Utc;
use serde::ser::SerializeStruct;
use serde::Serialize;
use tracing::span;
use tracing::span::Attributes;
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
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

use crate::alias::JsonValue;
use crate::ext::not;
use crate::ext::to_json_string;
use crate::ext::to_json_value;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

// -----------------------------------------------------------------------------
// Tracing service: Span field recorder
// -----------------------------------------------------------------------------
pub struct TracingContextLayer;

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

pub struct TracingJsonFormatter;

impl<S> FormatEvent<S, DefaultFields> for TracingJsonFormatter
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

        // TODO: temporary metrics from events
        let fields = to_json_value(event.field_map());
        #[cfg(feature = "metrics")]
        {
            event_to_metrics(&fields);
        }

        // parse metadata and event
        let log = TracingLog {
            timestamp: Utc::now(),
            level: meta.level().as_serde(),
            target: meta.target(),
            thread: std::thread::current(),
            fields,
            context,
        };

        writeln!(writer, "{}", to_json_string(&log))
    }
}

#[cfg(feature = "metrics")]
fn event_to_metrics(json: &JsonValue) {
    let Some(message) = json.as_object().and_then(|obj| obj.get("message")).and_then(|msg| msg.as_str()) else {
        return;
    };

    // jsonrpsee active connections
    let Some(message) = message.strip_prefix("Accepting new connection ") else {
        return;
    };
    let Some((current, _)) = message.split_once('/') else { return };
    let Ok(current) = current.parse::<u64>() else { return };
    metrics::set_rpc_requests_active(current);
}

#[derive(derive_new::new)]
struct TracingLog<'a> {
    timestamp: DateTime<Utc>,
    level: SerializeLevel<'a>,
    target: &'a str,
    thread: Thread,
    // fields: SerializeFieldMap<'a, Event<'a>>,
    fields: JsonValue,
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
pub struct TracingMinimalTimer;

impl FormatTime for TracingMinimalTimer {
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

/// Dynamic event logging based on the provided level.
///
/// <https://github.com/tokio-rs/tracing/issues/2730#issuecomment-1943022805>
#[macro_export]
macro_rules! event_with {
    ($lvl:ident, $($arg:tt)+) => {
        match $lvl {
            ::tracing::Level::ERROR => ::tracing::error!($($arg)+),
            ::tracing::Level::WARN => ::tracing::warn!($($arg)+),
            ::tracing::Level::INFO => ::tracing::info!($($arg)+),
            ::tracing::Level::DEBUG => ::tracing::debug!($($arg)+),
            ::tracing::Level::TRACE => ::tracing::trace!($($arg)+),
        }
    };
}

// -----------------------------------------------------------------------------
// Tracing functions
// -----------------------------------------------------------------------------

/// Creates a new possible unique correlation ID.
///
/// Uniqueness is not strictly necessary because traces are not stored permanently.
pub fn new_cid() -> String {
    pub const ALPHABET: [char; 36] = [
        '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't',
        'u', 'v', 'w', 'x', 'y', 'z',
    ];
    nanoid::nanoid!(8, &ALPHABET)
}

/// Emits an info message that a task was spawned to backgroud.
#[track_caller]
pub fn info_task_spawn(name: &str) {
    tracing::info!(parent: None, %name, "spawning task");
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
