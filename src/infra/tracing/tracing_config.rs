use std::str::FromStr;

use anyhow::anyhow;
use clap::Parser;
use display_json::DebugAsJson;
use opentelemetry_otlp::Protocol;

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct TracingConfig {
    #[arg(long = "tracing-log-format", env = "TRACING_LOG_FORMAT", default_value = "normal")]
    pub tracing_log_format: TracingLogFormat,

    #[arg(long = "tracing-url", alias = "tracing-collector-url", env = "TRACING_URL")]
    pub tracing_url: Option<String>,

    #[arg(long = "tracing-headers", env = "TRACING_HEADERS", value_delimiter = ',')]
    pub tracing_headers: Vec<String>,

    #[arg(long = "tracing-protocol", env = "TRACING_PROTOCOL", default_value = "grpc")]
    pub tracing_protocol: TracingProtocol,
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
