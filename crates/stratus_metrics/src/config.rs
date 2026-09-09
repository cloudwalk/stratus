use std::net::SocketAddr;

use clap::Parser;
use display_json::DebugAsJson;

use crate::metrics_for_consensus;
use crate::metrics_for_executor;
use crate::metrics_for_importer_online;
use crate::metrics_for_json_rpc;
use crate::metrics_for_kafka;
use crate::metrics_for_rocks;
use crate::metrics_for_storage_read;
use crate::metrics_for_storage_write;
use crate::set_node_mode_provider;

#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct MetricsConfig {
    /// Metrics exporter binding address.
    #[arg(long = "metrics-exporter-address", env = "METRICS_EXPORTER_ADDRESS", default_value = "0.0.0.0:9000")]
    pub metrics_exporter_address: SocketAddr,
}

impl MetricsConfig {
    /// Inits the application global metrics exporter.
    ///
    /// The `node_mode` provider is queried by every recorded metric, so it must
    /// read the current node mode instead of caching it.
    pub fn init(&self, node_mode: impl Fn() -> String + Send + Sync + 'static, service_name: &str, version: &str) -> anyhow::Result<()> {
        // node mode is read live by every recorded metric
        set_node_mode_provider(node_mode);

        tracing::info!(address = %self.metrics_exporter_address, "creating metrics exporter");

        // get metric definitions
        let mut metrics = Vec::new();
        metrics.extend(metrics_for_importer_online());
        metrics.extend(metrics_for_json_rpc());
        metrics.extend(metrics_for_executor());
        metrics.extend(metrics_for_storage_read());
        metrics.extend(metrics_for_storage_write());
        metrics.extend(metrics_for_rocks());
        metrics.extend(metrics_for_consensus());
        metrics.extend(metrics_for_kafka());

        // init metric exporter
        init_metrics_exporter(self.metrics_exporter_address, service_name, version);

        // init metric description (always after provider started)
        for metric in &metrics {
            metric.register_description();
        }

        Ok(())
    }
}

#[cfg(feature = "metrics")]
fn init_metrics_exporter(address: SocketAddr, service_name: &str, version: &str) {
    tracing::info!(%address, "creating prometheus metrics exporter");
    if let Err(e) = metrics_exporter_prometheus::PrometheusBuilder::new()
        .add_global_label("service", service_name)
        .add_global_label("version", version)
        .with_http_listener(address)
        .install()
    {
        tracing::error!(reason = ?e, %address, "failed to create metrics exporter");
    }
}

#[cfg(not(feature = "metrics"))]
fn init_metrics_exporter(_: SocketAddr, _: &str, _: &str) {
    tracing::info!("creating noop metrics exporter");
}
