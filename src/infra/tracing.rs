//! Tracing services.

use std::env;

use console_subscriber::ConsoleLayer;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::runtime;
use opentelemetry_sdk::trace;
use opentelemetry_sdk::Resource;
use tracing::Metadata;
use tracing::Subscriber;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::Context;
use tracing_subscriber::layer::Filter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

use crate::ext::not;
use crate::ext::spawn_named;

/// Init application global tracing.
pub async fn init_tracing(url: Option<&String>) {
    println!("creating tracing registry");

    // configure stdout layer
    let format_as_json = env::var_os("JSON_LOGS").is_some_and(|var| not(var.is_empty()));
    let stdout_layer = if format_as_json {
        println!("tracing registry enabling json logs");
        fmt::Layer::default()
            .json()
            .with_target(true)
            .with_thread_ids(true)
            .with_thread_names(true)
            .with_filter(EnvFilter::from_default_env())
            .boxed()
    } else {
        println!("tracing registry enabling text logs");
        fmt::Layer::default()
            .with_target(false)
            .with_thread_ids(false)
            .with_thread_names(false)
            .with_filter(EnvFilter::from_default_env())
            .boxed()
    };

    // configure opentelemetry layer
    let opentelemetry_layer = match url {
        Some(url) => {
            println!("tracing registry enabling opentelemetry exporter | url={}", url);
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
            println!("tracing registry NOT enabling opentelemetry exporter");
            None
        }
    };

    // init tokio console registry
    println!("tracing registry enabling tokio console");
    let (console_layer, console_server) = ConsoleLayer::builder().with_default_env().build();
    let console_layer = console_layer.with_filter(TokioConsoleFilter);

    // init tokio console server
    spawn_named("console::grpc-server", async move {
        if let Err(e) = console_server.serve().await {
            tracing::error!(reason = ?e, "failed to create tokio-console server");
        };
    });

    // init registry
    tracing_subscriber::registry()
        .with(stdout_layer)
        .with(opentelemetry_layer)
        .with(console_layer)
        .init();
}

/// Workaround filter for `tokio-console` panicking in debug mode when an event is not an event or span.
///
/// Can be removed after this PR is merged: https://github.com/tokio-rs/console/pull/554
struct TokioConsoleFilter;

impl<S> Filter<S> for TokioConsoleFilter
where
    S: Subscriber,
{
    fn enabled(&self, meta: &Metadata<'_>, _: &Context<'_, S>) -> bool {
        meta.is_span() || meta.is_event()
    }

    fn callsite_enabled(&self, meta: &'static Metadata<'static>) -> tracing::subscriber::Interest {
        if not(meta.is_span()) && not(meta.is_event()) {
            return tracing::subscriber::Interest::never();
        }
        tracing::subscriber::Interest::always()
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
