use std::collections::HashMap;
use std::io::stdout;
use std::io::IsTerminal;
use std::net::SocketAddr;
use std::str::FromStr;

use anyhow::anyhow;
use clap::Parser;
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
use tonic::metadata::MetadataKey;
use tonic::metadata::MetadataMap;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

use crate::ext::spawn_named;
use crate::infra::build_info;
use crate::infra::sentry::SentryConfig;
use crate::infra::tracing::TracingContextLayer;
use crate::infra::tracing::TracingJsonFormatter;
use crate::infra::tracing::TracingMinimalTimer;

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct TracingConfig {
    /// OpenTelemetry server URL.
    #[arg(long = "tracing-url", alias = "tracing-collector-url", env = "TRACING_URL")]
    pub tracing_url: Option<String>,

    /// OpenTelemetry server communication protocol.
    #[arg(long = "tracing-protocol", env = "TRACING_PROTOCOL", default_value = "grpc")]
    pub tracing_protocol: TracingProtocol,

    /// OpenTelemetry additional HTTP headers or GRPC metadata.
    #[arg(long = "tracing-headers", env = "TRACING_HEADERS", value_delimiter = ',')]
    pub tracing_headers: Vec<String>,

    /// How tracing events will be formatted when displayed in stdout.
    #[arg(long = "tracing-log-format", env = "TRACING_LOG_FORMAT", default_value = "normal")]
    pub tracing_log_format: TracingLogFormat,

    // Tokio Console GRPC server binding address.
    #[arg(long = "tokio-console-address", env = "TRACING_TOKIO_CONSOLE_ADDRESS")]
    pub tracing_tokio_console_address: Option<SocketAddr>,
}

impl TracingConfig {
    /// Inits application global tracing registry.
    ///
    /// Uses println! to have information available in stdout before tracing is initialized.
    pub fn init(&self, sentry_config: &Option<SentryConfig>) -> anyhow::Result<()> {
        match self.create_subscriber(sentry_config).try_init() {
            Ok(()) => Ok(()),
            Err(e) => {
                println!("failed to create tracing registry | reason={:?}", e);
                Err(e.into())
            }
        }
    }
    pub fn create_subscriber(&self, sentry_config: &Option<SentryConfig>) -> impl SubscriberInitExt {
        println!("creating tracing registry");

        // configure tracing context layer
        println!("tracing registry: enabling tracing context recorder");
        let tracing_context_layer = TracingContextLayer.with_filter(EnvFilter::from_default_env());

        // configure stdout log layer
        let enable_ansi = stdout().is_terminal();
        println!(
            "tracing registry: enabling console logs | format={} ansi={}",
            self.tracing_log_format, enable_ansi
        );
        let stdout_layer = match self.tracing_log_format {
            TracingLogFormat::Json => fmt::Layer::default()
                .event_format(TracingJsonFormatter)
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
        let opentelemetry_layer = match &self.tracing_url {
            Some(url) => {
                let tracer = opentelemetry_tracer(url, self.tracing_protocol, &self.tracing_headers);
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
        let sentry_layer = match &sentry_config {
            Some(sentry_config) => {
                println!("tracing registry: enabling sentry exporter | url={}", sentry_config.sentry_url);
                let layer = sentry_tracing::layer().with_filter(EnvFilter::from_default_env());
                Some(layer)
            }
            None => {
                println!("tracing registry: skipping sentry exporter");
                None
            }
        };

        // configure tokio-console layer
        let tokio_console_layer = match self.tracing_tokio_console_address {
            Some(tokio_console_address) => {
                println!("tracing registry: enabling tokio console exporter | address={}", tokio_console_address);

                let (console_layer, console_server) = ConsoleLayer::builder().with_default_env().server_addr(tokio_console_address).build();
                spawn_named("console::grpc-server", async move {
                    if let Err(e) = console_server.serve().await {
                        tracing::error!(reason = ?e, address = %tokio_console_address, "failed to create tokio-console server");
                    };
                });
                Some(console_layer)
            }
            None => {
                println!("tracing registry: skipping tokio-console exporter");
                None
            }
        };

        tracing_subscriber::registry()
            .with(tracing_context_layer)
            .with(stdout_layer)
            .with(opentelemetry_layer)
            .with(sentry_layer)
            .with(tokio_console_layer)
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
// Protocol
// -----------------------------------------------------------------------------

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
// LogFormat
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracing_config_with_json_format() {
        let config = TracingConfig {
            tracing_url: None,
            tracing_protocol: TracingProtocol::Grpc,
            tracing_headers: vec![],
            tracing_log_format: TracingLogFormat::Json,
            tracing_tokio_console_address: None,
        };
        config.create_subscriber(&None);
    }

    #[test]
    fn test_tracing_config_with_minimal_format() {
        let config = TracingConfig {
            tracing_url: None,
            tracing_protocol: TracingProtocol::Grpc,
            tracing_headers: vec![],
            tracing_log_format: TracingLogFormat::Minimal,
            tracing_tokio_console_address: None,
        };
        config.create_subscriber(&None);
    }

    #[test]
    fn test_tracing_config_with_normal_format() {
        let config = TracingConfig {
            tracing_url: None,
            tracing_protocol: TracingProtocol::Grpc,
            tracing_headers: vec![],
            tracing_log_format: TracingLogFormat::Normal,
            tracing_tokio_console_address: None,
        };
        config.create_subscriber(&None);
    }

    #[test]
    fn test_tracing_config_with_verbose_format() {
        let config = TracingConfig {
            tracing_url: None,
            tracing_protocol: TracingProtocol::Grpc,
            tracing_headers: vec![],
            tracing_log_format: TracingLogFormat::Verbose,
            tracing_tokio_console_address: None,
        };
        config.create_subscriber(&None);
    }

    #[tokio::test]
    async fn test_tracing_config_with_opentelemetry() {
        let config = TracingConfig {
            tracing_url: Some("http://localhost:4317".to_string()),
            tracing_protocol: TracingProtocol::Grpc,
            tracing_headers: vec![],
            tracing_log_format: TracingLogFormat::Normal,
            tracing_tokio_console_address: None,
        };
        config.create_subscriber(&None);
    }

    #[test]
    fn test_tracing_config_with_sentry() {
        let sentry_config = SentryConfig {
            sentry_url: "http://localhost:1234".to_string(),
        };
        let config = TracingConfig {
            tracing_url: None,
            tracing_protocol: TracingProtocol::Grpc,
            tracing_headers: vec![],
            tracing_log_format: TracingLogFormat::Normal,
            tracing_tokio_console_address: None,
        };
        config.create_subscriber(&Some(sentry_config));
    }

    #[tokio::test]
    async fn test_tracing_config_with_tokio_console() {
        let config = TracingConfig {
            tracing_url: None,
            tracing_protocol: TracingProtocol::Grpc,
            tracing_headers: vec![],
            tracing_log_format: TracingLogFormat::Normal,
            tracing_tokio_console_address: Some("127.0.0.1:6669".parse().unwrap()),
        };
        config.create_subscriber(&None);
    }

    #[test]
    fn test_tracing_protocol_from_str() {
        assert_eq!(
            TracingProtocol::from_str("grpc").unwrap(),
            TracingProtocol::Grpc
        );
        assert_eq!(
            TracingProtocol::from_str("http-binary").unwrap(),
            TracingProtocol::HttpBinary
        );
        assert_eq!(
            TracingProtocol::from_str("http-json").unwrap(),
            TracingProtocol::HttpJson
        );
        assert!(TracingProtocol::from_str("invalid").is_err());
    }

    #[test]
    fn test_tracing_protocol_display() {
        assert_eq!(TracingProtocol::Grpc.to_string(), "grpc");
        assert_eq!(TracingProtocol::HttpBinary.to_string(), "http-binary");
        assert_eq!(TracingProtocol::HttpJson.to_string(), "http-json");
    }

    #[test]
    fn test_tracing_protocol_into_protocol() {
        assert_eq!(Protocol::from(TracingProtocol::Grpc), Protocol::Grpc);
        assert_eq!(Protocol::from(TracingProtocol::HttpBinary), Protocol::HttpBinary);
        assert_eq!(Protocol::from(TracingProtocol::HttpJson), Protocol::HttpJson);
    }
}
