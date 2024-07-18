use std::net::SocketAddr;
use std::stringify;

use crate::infra::metrics::metrics_for_consensus;
use crate::infra::metrics::metrics_for_evm;
use crate::infra::metrics::metrics_for_executor;
use crate::infra::metrics::metrics_for_external_relayer;
use crate::infra::metrics::metrics_for_importer_online;
use crate::infra::metrics::metrics_for_json_rpc;
use crate::infra::metrics::metrics_for_rocks;
use crate::infra::metrics::metrics_for_storage_read;
use crate::infra::metrics::metrics_for_storage_write;

/// Init application global metrics.
///
/// Default configuration runs metrics exporter on port 9000.
pub fn init_metrics(address: SocketAddr) -> anyhow::Result<()> {
    tracing::info!(%address, "creating metrics exporter");

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
    metrics.extend(metrics_for_external_relayer());

    // init metric exporter
    init_metrics_exporter(address);

    // init metric description (always after provider started)
    for metric in &metrics {
        metric.register_description();
    }

    Ok(())
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
