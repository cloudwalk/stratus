use std::net::SocketAddr;
use std::stringify;
use std::sync::OnceLock;

use clap::Parser;
use display_json::DebugAsJson;

use crate::infra::metrics::metrics_for_consensus;
use crate::infra::metrics::metrics_for_evm;
use crate::infra::metrics::metrics_for_executor;
use crate::infra::metrics::metrics_for_importer_online;
use crate::infra::metrics::metrics_for_json_rpc;
use crate::infra::metrics::metrics_for_kafka;
use crate::infra::metrics::metrics_for_rocks;
use crate::infra::metrics::metrics_for_storage_read;
use crate::infra::metrics::metrics_for_storage_write;

#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct MetricsConfig {
    /// Metrics exporter binding address.
    #[arg(long = "metrics-exporter-address", env = "METRICS_EXPORTER_ADDRESS", default_value = "0.0.0.0:9000")]
    pub metrics_exporter_address: SocketAddr,
    
    /// Enable metrics sampling for metrics that have sampling configured.
    /// When disabled, all metrics are recorded regardless of @sample() annotations.
    #[arg(long = "metrics-enable-sampling", env = "METRICS_ENABLE_SAMPLING", default_value = "true")]
    pub enable_sampling: bool,
}

// Global sampling state
static SAMPLING_ENABLED: OnceLock<bool> = OnceLock::new();

impl MetricsConfig {
    /// Inits application global metrics exporter.
    pub fn init(&self) -> anyhow::Result<()> {
        tracing::info!(
            address = %self.metrics_exporter_address,
            sampling_enabled = %self.enable_sampling,
            "creating metrics exporter"
        );

        // Initialize global sampling state
        SAMPLING_ENABLED.set(self.enable_sampling).map_err(|_| {
            anyhow::anyhow!("Failed to initialize sampling configuration")
        })?;

        // get metric definitions
        let mut metrics = Vec::new();
        metrics.extend(metrics_for_importer_online());
        metrics.extend(metrics_for_json_rpc());
        metrics.extend(metrics_for_executor());
        metrics.extend(metrics_for_evm());
        metrics.extend(metrics_for_storage_read());
        metrics.extend(metrics_for_storage_write());
        metrics.extend(metrics_for_rocks());
        metrics.extend(metrics_for_consensus());
        metrics.extend(metrics_for_kafka());

        // init metric exporter
        init_metrics_exporter(self.metrics_exporter_address);

        // init metric description (always after provider started)
        for metric in &metrics {
            metric.register_description();
        }

        Ok(())
    }
}

/// Check if sampling is globally enabled
pub fn is_sampling_enabled() -> bool {
    SAMPLING_ENABLED.get().copied().unwrap_or(true)
}

#[cfg(feature = "metrics")]
fn init_metrics_exporter(address: SocketAddr) {
    tracing::info!(%address, "creating prometheus metrics exporter");
    if let Err(e) = metrics_exporter_prometheus::PrometheusBuilder::new()
        .add_global_label("service", crate::infra::build_info::service_name())
        .add_global_label("version", crate::infra::build_info::version())
        .with_http_listener(address)
        .install()
    {
        tracing::error!(reason = ?e, %address, "failed to create metrics exporter");
    }
}

#[cfg(not(feature = "metrics"))]
fn init_metrics_exporter(_: SocketAddr) {
    tracing::info!("creating noop metrics exporter");
}
