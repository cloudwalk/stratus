use std::net::SocketAddr;

use clap::Parser;
use display_json::DebugAsJson;
#[cfg(feature = "metrics")]
use metrics_exporter_prometheus::PrometheusBuilder;
#[cfg(feature = "metrics")]
use metrics_tracing_context::TracingContextLayer as MetricsTracingContextLayer;
#[cfg(feature = "metrics")]
use metrics_util::layers::Layer as MetricsLayerExt;

use crate::infra::metrics::metrics_for_consensus;
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
}

impl MetricsConfig {
    /// Inits application global metrics exporter.
    pub fn init(&self) -> anyhow::Result<()> {
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
        init_metrics_exporter(self.metrics_exporter_address);

        // init metric description (always after provider started)
        for metric in &metrics {
            metric.register_description();
        }

        Ok(())
    }
}

#[cfg(feature = "metrics")]
fn init_metrics_exporter(address: SocketAddr) {
    tracing::info!(%address, "creating prometheus metrics exporter");

    let builder = PrometheusBuilder::new()
        .add_global_label("service", crate::infra::build_info::service_name())
        .add_global_label("version", crate::infra::build_info::version())
        .with_http_listener(address);

    if let Err(e) = install_metrics_tracing_recorder(builder) {
        tracing::error!(reason = ?e, %address, "failed to create metrics exporter");
    }
}

#[cfg(feature = "metrics")]
fn install_metrics_tracing_recorder(builder: PrometheusBuilder) -> anyhow::Result<()> {
    let (recorder, exporter) = builder.build()?;
    tokio::spawn(exporter);

    let recorder = MetricsTracingContextLayer::only_allow(["rpc_client", "rpc_method", "point_in_time"]).layer(recorder);
    metrics::set_global_recorder(recorder)?;

    Ok(())
}

#[cfg(not(feature = "metrics"))]
fn init_metrics_exporter(_: SocketAddr) {
    tracing::info!("creating noop metrics exporter");
}
