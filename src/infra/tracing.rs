//! Tracing services.

use std::collections::VecDeque;
use std::fmt;
use std::io;
use std::io::stdout;

use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::runtime;
use opentelemetry_sdk::trace;
use opentelemetry_sdk::Resource;
use rustc_hash::FxHashMap;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
use uuid::Uuid;

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
        .json()
        .with_writer(LogFormatter::new)
        .with_target(false)
        .with_ansi(false)
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
            tracing_subscriber::fmt::Layer::default()
                .with_target(false)
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_filter(EnvFilter::from_default_env()),
        )
        .try_init()
        .unwrap();
}

/// Our logger formatter
pub struct LogFormatter {
    line_buf: VecDeque<u8>,
}

impl LogFormatter {
    fn new() -> Self {
        Self { line_buf: VecDeque::new() }
    }

    /// Provide the tracing span used to detect the context ID.
    pub fn new_context_id_span() -> tracing::Span {
        tracing::info_span!("context_id", context_id = Uuid::new_v4().to_string())
    }
}

impl io::Write for LogFormatter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.line_buf.extend(buf);

        if let Some(position) = self.line_buf.iter().position(|&byt| byt == b'\n') {
            let line = self.line_buf.drain(..=position).collect::<Vec<u8>>();
            let parsed_log: Log = serde_json::from_reader(line.as_slice())?;
            let out = parsed_log.to_string();
            writeln!(stdout(), "{out}")?;
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        stdout().flush()
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Log {
    timestamp: String,
    level: String,
    fields: FxHashMap<String, JsonValue>,
    spans: Vec<FxHashMap<String, String>>,
}

impl fmt::Display for Log {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let Self {
            timestamp,
            level,
            fields,
            spans,
        } = self;

        write!(f, "{timestamp} {level} ")?;

        let context_id = spans.iter().find_map(|kv| if kv.len() >= 2 { kv.get("context_id") } else { None });

        if let Some(context_id) = context_id {
            write!(f, "[{context_id}] ")?;
        }

        for span in spans {
            let name = span.get("name");
            let name = name.map(String::as_str).unwrap_or("?UNKNOWN?");

            // filter out context_id, as it's was already treated
            if name == "context_id" {
                continue;
            }

            write!(f, "{name}:")?;
        }

        for (key, value) in fields {
            write!(f, " {key}={value}")?;
        }

        Ok(())
    }
}
