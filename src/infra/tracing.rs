//! Tracing services.

use std::env;
use std::env::VarError;
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

use crate::ext::named_spawn;
use crate::GlobalState;

/// Init application global tracing.
pub async fn init_tracing(url: Option<&String>, console_address: SocketAddr) -> anyhow::Result<()> {
    println!("creating global tracing registry");

    // configure stdout log layer
    let log_format = env::var("LOG_FORMAT").map(|x| x.trim().to_lowercase());
    let enable_ansi = stdout().is_terminal();

    let stdout_layer = match log_format.as_deref() {
        Ok("json") => {
            println!("tracing registry: enabling json logs");
            fmt::Layer::default()
                .json()
                .with_target(true)
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_filter(EnvFilter::from_default_env())
                .boxed()
        }
        Ok("verbose") | Ok("full") => {
            println!("tracing registry: enabling verbose text logs");
            fmt::Layer::default()
                .with_ansi(enable_ansi)
                .with_target(true)
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_filter(EnvFilter::from_default_env())
                .boxed()
        }
        Ok("minimal") => {
            println!("tracing registry: enabling minimal text logs");
            fmt::Layer::default()
                .with_thread_ids(false)
                .with_thread_names(false)
                .with_target(false)
                .with_ansi(enable_ansi)
                .with_timer(MinimalTimer)
                .with_filter(EnvFilter::from_default_env())
                .boxed()
        }
        Ok("normal") | Err(VarError::NotPresent) => {
            println!("tracing registry: enabling normal text logs");
            fmt::Layer::default().with_ansi(enable_ansi).with_filter(EnvFilter::from_default_env()).boxed()
        }
        Ok(unexpected) => panic!("unexpected `LOG_FORMAT={unexpected}`"),
        Err(e) => panic!("invalid utf-8 in `LOG_FORMAT`: {e}"),
    };

    // configure opentelemetry layer
    let opentelemetry_layer = match url {
        Some(url) => {
            println!("tracing registry: enabling opentelemetry exporter | url={}", url);
            let tracer_config = trace::config().with_resource(Resource::new(vec![KeyValue::new("service.name", "stratus")]));
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
    println!("tracing registry: enabling tracing layer");
    let sentry_layer = sentry_tracing::layer().with_filter(EnvFilter::from_default_env());

    // configure tokio-console layer
    println!("tracing registry: enabling tokio console | address={}", console_address);
    let (console_layer, console_server) = ConsoleLayer::builder().with_default_env().server_addr(console_address).build();
    named_spawn("console::grpc-server", async move {
        if let Err(e) = console_server.serve().await {
            let message = GlobalState::shutdown_from("tracing-init", &format!("failed to create tokio-console server at {}", console_address));
            tracing::error!(reason = ?e, address = %console_address, %message);
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
            println!("failed to create global tracing registry | reason={:?}", e);
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
