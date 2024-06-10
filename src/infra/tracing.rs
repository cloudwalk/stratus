//! Tracing services.

use std::io::stdout;
use std::io::IsTerminal;
use std::net::SocketAddr;

use chrono::Local;
use console_subscriber::ConsoleLayer;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::runtime;
use opentelemetry_sdk::trace;
use opentelemetry_sdk::Resource;
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

use crate::config::LogFormat;
use crate::ext::binary_name;
use crate::ext::named_spawn;

/// Init application tracing.
pub async fn init_tracing(log_format: LogFormat, opentelemetry_url: Option<&str>, tokio_console_address: SocketAddr) -> anyhow::Result<()> {
    println!("creating tracing registry");

    // configure stdout log layer
    let enable_ansi = stdout().is_terminal();

    println!("tracing registry: enabling console logs | format={} ansi={}", log_format, enable_ansi);
    let stdout_layer = match log_format {
        LogFormat::Json => fmt::Layer::default()
            .json()
            .with_target(true)
            .with_thread_ids(true)
            .with_thread_names(true)
            .with_filter(EnvFilter::from_default_env())
            .boxed(),
        LogFormat::Minimal => fmt::Layer::default()
            .with_thread_ids(false)
            .with_thread_names(false)
            .with_target(false)
            .with_ansi(enable_ansi)
            .with_timer(MinimalTimer)
            .with_filter(EnvFilter::from_default_env())
            .boxed(),
        LogFormat::Normal => fmt::Layer::default().with_ansi(enable_ansi).with_filter(EnvFilter::from_default_env()).boxed(),
        LogFormat::Verbose => fmt::Layer::default()
            .with_ansi(enable_ansi)
            .with_target(true)
            .with_thread_ids(true)
            .with_thread_names(true)
            .with_filter(EnvFilter::from_default_env())
            .boxed(),
    };

    // configure opentelemetry layer
    let opentelemetry_layer = match opentelemetry_url {
        Some(url) => {
            let service_name = format!("stratus-{}", binary_name());
            println!("tracing registry: enabling opentelemetry exporter | url={} service={}", url, service_name);
            let tracer_config = trace::config().with_resource(Resource::new(vec![KeyValue::new("service.name", service_name)]));
            let tracer_exporter = opentelemetry_otlp::new_exporter().tonic().with_endpoint(url);

            let tracer = opentelemetry_otlp::new_pipeline()
                .tracing()
                .with_exporter(tracer_exporter)
                .with_trace_config(tracer_config)
                .install_batch(runtime::Tokio)
                .unwrap();

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
    println!("tracing registry: enabling sentry exporter");
    let sentry_layer = sentry_tracing::layer().with_filter(EnvFilter::from_default_env());

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

struct MinimalTimer;

impl FormatTime for MinimalTimer {
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
