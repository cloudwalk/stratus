//! Metrics services.
#![cfg(feature = "metrics")]

use std::net::SocketAddr;
use std::stringify;

use metrics_exporter_prometheus::PrometheusBuilder;

use crate::infra::build_info;
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

    // init exporter
    if let Err(e) = PrometheusBuilder::new()
        .add_global_label("service", build_info::service_name())
        .add_global_label("version", build_info::version())
        .with_http_listener(address)
        .install()
    {
        tracing::error!(reason = ?e, %address, "failed to create metrics exporter");
    }

    // init metric description (always after provider started)
    for metric in &metrics {
        metric.register_description();
    }

    Ok(())
}
