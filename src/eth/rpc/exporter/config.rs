use clap::Parser;
use display_json::DebugAsJson;

use crate::eth::rpc::exporter::ExporterRuntime;
use crate::eth::rpc::exporter::ExporterRuntimeConfig;

#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct ExporterConfig {
    /// Number of Tokio worker threads dedicated to the exporter runtime.
    #[arg(id = "exporter.async_threads", long = "exporter-async-threads", default_value = "4")]
    pub async_threads: usize,

    /// Number of blocking threads on the exporter runtime, reserved for importer-facing requests.
    #[arg(id = "exporter.blocking_threads", long = "exporter-blocking-threads", default_value = "64")]
    pub blocking_threads: usize,
}

#[cfg(test)]
impl Default for ExporterConfig {
    fn default() -> Self {
        Self {
            async_threads: 4,
            blocking_threads: 64,
        }
    }
}

impl ExporterConfig {
    /// Initializes the dedicated exporter runtime for importer-facing requests.
    pub async fn init(&self) -> anyhow::Result<ExporterRuntime> {
        tracing::info!(
            async_threads = self.async_threads,
            blocking_threads = self.blocking_threads,
            "creating dedicated exporter runtime"
        );

        let runtime = ExporterRuntime::start(ExporterRuntimeConfig {
            async_threads: self.async_threads,
            blocking_threads: self.blocking_threads,
        })
        .await?;

        Ok(runtime)
    }
}
