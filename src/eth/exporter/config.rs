use clap::Parser;
use display_json::DebugAsJson;

use crate::eth::exporter::ExporterRuntime;
use crate::eth::exporter::ExporterRuntimeConfig;

fn parse_thread_count(value: &str) -> Result<usize, String> {
    let value = value.parse::<usize>().map_err(|error| error.to_string())?;
    if value == 0 {
        return Err("thread count must be greater than zero".to_owned());
    }
    Ok(value)
}

#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct ExporterConfig {
    /// Enables the dedicated exporter runtime, which isolates importer-facing requests from the main blocking pool.
    #[arg(long = "exporter-enabled", env = "EXPORTER_ENABLED", default_value = "true")]
    pub exporter_enabled: bool,

    /// Number of Tokio worker threads dedicated to the exporter runtime.
    #[arg(
        long = "exporter-async-threads",
        env = "EXPORTER_ASYNC_THREADS",
        default_value = "4",
        value_parser = parse_thread_count
    )]
    pub exporter_async_threads: usize,

    /// Number of blocking threads on the exporter runtime, reserved for importer-facing requests.
    #[arg(
        long = "exporter-blocking-threads",
        env = "EXPORTER_BLOCKING_THREADS",
        default_value = "64",
        value_parser = parse_thread_count
    )]
    pub exporter_blocking_threads: usize,
}

impl ExporterConfig {
    /// Initializes the dedicated exporter runtime for importer-facing requests.
    pub async fn init(&self) -> anyhow::Result<Option<ExporterRuntime>> {
        if !self.exporter_enabled {
            tracing::info!("exporter disabled, importer-facing requests will use the main blocking pool");
            return Ok(None);
        }

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

        Ok(Some(runtime))
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::ExporterConfig;

    #[test]
    fn exporter_config_defaults_to_enabled() {
        let config = ExporterConfig::parse_from(vec!["program"]);
        assert!(config.exporter_enabled);
        assert_eq!(config.exporter_async_threads, 2);
        assert_eq!(config.exporter_blocking_threads, 32);
    }

    #[test]
    fn exporter_config_rejects_zero_threads() {
        let error = ExporterConfig::try_parse_from(vec!["program", "--exporter-blocking-threads", "0"]);
        assert!(error.is_err());
    }
}
