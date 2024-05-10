//! Tracing services.

use std::env;

use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::runtime;
use opentelemetry_sdk::trace;
use opentelemetry_sdk::Resource;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

use crate::ext::not;

/// Init application global tracing.
pub fn init_tracing(url: Option<&String>) {
    println!("starting tracing");
    match url {
        Some(url) => init_opentelemetry_tracing(url),
        None => init_stdout_tracing(),
    }

    tracing::info!("started tracing");
}

fn init_stdout_tracing() {
    // if tracing level not configured, set default
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "stratus=debug");
    }

    let subscriber = tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_ansi(false)
        .with_env_filter(EnvFilter::from_default_env());

    let json_logs = env::var_os("JSON_LOGS").is_some_and(|var| not(var.is_empty()));
    if json_logs {
        subscriber.json().init();
    } else {
        subscriber.compact().init();
    }
}

// Only works if run inside the tokio runtime
fn init_opentelemetry_tracing(url: &str) {
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(opentelemetry_otlp::new_exporter().tonic().with_endpoint(url))
        .with_trace_config(trace::config().with_resource(Resource::new(vec![KeyValue::new("service.name", "stratus-tracing")])))
        .install_batch(runtime::Tokio)
        .unwrap();

    let opentelemetry = tracing_opentelemetry::layer().with_tracer(tracer);

    tracing_subscriber::registry()
        .with(opentelemetry)
        // Continue logging to stdout
        .with(
            fmt::Layer::default()
                .with_target(false)
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_filter(EnvFilter::from_default_env()),
        )
        .try_init()
        .unwrap();
}
