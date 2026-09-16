use clap::Parser;
use display_json::DebugAsJson;

use crate::eth::exporter::ExporterRuntime;
use crate::eth::exporter::ExporterRuntimeConfig;

#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct ExporterConfig {
    /// Number of Tokio worker threads dedicated to the exporter runtime.
    #[arg(long = "exporter-async-threads", env = "EXPORTER_ASYNC_THREADS", default_value = "4")]
    pub exporter_async_threads: usize,

    /// Number of blocking threads on the exporter runtime, reserved for importer-facing requests.
    #[arg(long = "exporter-blocking-threads", env = "EXPORTER_BLOCKING_THREADS", default_value = "64")]
    pub exporter_blocking_threads: usize,
}

impl ExporterConfig {
    /// Initializes the dedicated exporter runtime for importer-facing requests.
    pub async fn init(&self) -> anyhow::Result<ExporterRuntime> {
        tracing::info!(
            exporter_async_threads = self.exporter_async_threads,
            exporter_blocking_threads = self.exporter_blocking_threads,
            "creating dedicated exporter runtime"
        );

        let runtime = ExporterRuntime::start(ExporterRuntimeConfig {
            async_threads: self.exporter_async_threads,
            blocking_threads: self.exporter_blocking_threads,
        })
        .await?;

        Ok(runtime)
    }
}
