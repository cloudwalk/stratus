//! Tracing services.

use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{runtime, trace, Resource};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};
//use tracing_subscriber::EnvFilter;

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
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "stratus=debug");
    }

    tracing_subscriber::fmt()
        .compact()
        .with_target(false)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_env_filter(EnvFilter::from_default_env())
        .try_init()
        .expect("failed to start tracing");
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
