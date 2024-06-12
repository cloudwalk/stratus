//! Tracing services.

use std::collections::HashMap;
use std::io::stdout;
use std::io::IsTerminal;
use std::net::SocketAddr;
use std::str::FromStr;

use anyhow::anyhow;
use chrono::Local;
use console_subscriber::ConsoleLayer;
use display_json::DebugAsJson;
use itertools::Itertools;
use opentelemetry::KeyValue;
use opentelemetry_otlp::Protocol;
use opentelemetry_otlp::SpanExporterBuilder;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::runtime;
use opentelemetry_sdk::trace;
use opentelemetry_sdk::trace::Tracer as SdkTracer;
use opentelemetry_sdk::Resource as SdkResource;
use tonic::metadata::MetadataKey;
use tonic::metadata::MetadataMap;
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

use crate::config::TracingConfig;
use crate::ext::binary_name;
use crate::ext::named_spawn;

/// Init application tracing.
pub async fn init_tracing(config: &TracingConfig, sentry_url: Option<&str>, tokio_console_address: SocketAddr) -> anyhow::Result<()> {
    println!("creating tracing registry");

    // configure stdout log layer
    let enable_ansi = stdout().is_terminal();

    println!(
        "tracing registry: enabling console logs | format={} ansi={}",
        config.tracing_log_format, enable_ansi
    );
    let stdout_layer = match config.tracing_log_format {
        TracingLogFormat::Json => fmt::Layer::default()
            .json()
            .with_target(true)
            .with_thread_ids(true)
            .with_thread_names(true)
            .with_filter(EnvFilter::from_default_env())
            .boxed(),
        TracingLogFormat::Minimal => fmt::Layer::default()
            .with_thread_ids(false)
            .with_thread_names(false)
            .with_target(false)
            .with_ansi(enable_ansi)
            .with_timer(TracingMinimalTimer)
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
    named_spawn("console::grpc-server", async move {
        if let Err(e) = console_server.serve().await {
            tracing::error!(reason = ?e, address = %tokio_console_address, "failed to create tokio-console server");
        };
    });

    // init registry
    let result = tracing_subscriber::registry()
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
    let service_name = format!("stratus-{}", binary_name());
    println!(
        "tracing registry: enabling opentelemetry exporter | url={} protocol={} headers={} service={}",
        url,
        protocol,
        headers.len(),
        service_name
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

    let tracer_config = trace::config()
        .with_resource(SdkResource::new(vec![KeyValue::new("service.name", service_name)]));

    opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(tracer_exporter)
        .with_trace_config(tracer_config)
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
// Tracing services
// -----------------------------------------------------------------------------

struct TracingMinimalTimer;

impl FormatTime for TracingMinimalTimer {
    fn format_time(&self, w: &mut fmt::format::Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", Local::now().time().format("%H:%M:%S%.3f"))
    }
}

// -----------------------------------------------------------------------------
// Tracing functions
// -----------------------------------------------------------------------------

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
