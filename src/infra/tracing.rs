//! Tracing services.

use std::env;

use console_subscriber::ConsoleLayer;
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
pub async fn init_tracing(url: Option<&String>) {
    println!("starting tracing");

    // configure stdout layer
    let format_as_json = env::var_os("JSON_LOGS").is_some_and(|var| not(var.is_empty()));
    let stdout_layer = if format_as_json {
        println!("tracing enabling json logs");
        fmt::Layer::default()
            .json()
            .with_target(true)
            .with_thread_ids(true)
            .with_thread_names(true)
            .with_filter(EnvFilter::from_default_env())
            .boxed()
    } else {
        println!("tracing enabling text logs");
        fmt::Layer::default()
            .with_target(false)
            .with_thread_ids(true)
            .with_thread_names(true)
            .with_filter(EnvFilter::from_default_env())
            .boxed()
    };

    // configure tokio console layer
    println!("tracing enabling tokio console");
    let (console_layer, console_server) = ConsoleLayer::builder().with_default_env().build();

    // configure opentelemetry layer
    let opentelemetry_layer = match url {
        Some(url) => {
            println!("tracing enabling opentelemetry");
            let tracer = opentelemetry_otlp::new_pipeline()
                .tracing()
                .with_exporter(opentelemetry_otlp::new_exporter().tonic().with_endpoint(url))
                .with_trace_config(trace::config().with_resource(Resource::new(vec![KeyValue::new("service.name", "stratus")])))
                .install_batch(runtime::Tokio)
                .unwrap();

            Some(tracing_opentelemetry::layer().with_tracked_inactivity(false).with_tracer(tracer))
        }
        None => None,
    };

    // init registry
    tracing_subscriber::registry()
        .with(stdout_layer)
        .with(console_layer)
        .with(opentelemetry_layer)
        .init();

    // init tokio console server
    tokio::task::Builder::new()
        .name("console::grpc-server")
        .spawn(async move {
            if let Err(e) = console_server.serve().await {
                tracing::error!(reason = ?e, "failed to start tokio-console server");
            };
        })
        .expect("spawning tokio-console grpc server should not fail");

    tracing::info!("started tracing");
}
