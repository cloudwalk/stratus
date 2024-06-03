//! Metrics services.
#![cfg(feature = "metrics")]

use std::net::SocketAddr;
use std::stringify;

use metrics_exporter_prometheus::Matcher;
use metrics_exporter_prometheus::PrometheusBuilder;

use crate::config::MetricsHistogramKind;
use crate::infra::metrics::metrics_for_consensus;
use crate::infra::metrics::metrics_for_evm;
use crate::infra::metrics::metrics_for_executor;
use crate::infra::metrics::metrics_for_external_relayer;
use crate::infra::metrics::metrics_for_importer_online;
use crate::infra::metrics::metrics_for_json_rpc;
use crate::infra::metrics::metrics_for_rocks;
use crate::infra::metrics::metrics_for_storage_read;
use crate::infra::metrics::metrics_for_storage_write;

/// Default bucket for duration based metrics.
const BUCKET_FOR_DURATION: [f64; 37] = [
    0.0001, 0.0002, 0.0003, 0.0004, 0.0005, 0.0006, 0.0007, 0.0008, 0.0009, // 0.1ms to 0.9ms
    0.001, 0.002, 0.003, 0.004, 0.005, 0.006, 0.007, 0.008, 0.009, // 1ms to 9ms
    0.01, 0.02, 0.03, 0.04, 0.05, 0.06, 0.07, 0.08, 0.09, // 10ms to 90ms
    0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, // 100ms to 900ms
    1.,  // 1s or more
];

/// Init application global metrics.
///
/// Default configuration runs metrics exporter on port 9000.
pub fn init_metrics(metrics_exporter_address: SocketAddr, histogram_kind: MetricsHistogramKind) {
    tracing::info!("creating metrics exporter");

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
    let mut builder = PrometheusBuilder::new().with_http_listener(metrics_exporter_address);

    // init buckets
    if histogram_kind == MetricsHistogramKind::Histogram {
        builder = builder.set_buckets(&BUCKET_FOR_DURATION).unwrap();
        for metric in &metrics {
            if metric.has_custom_buckets() {
                builder = builder.set_buckets_for_metric(Matcher::Full(metric.name.to_string()), &metric.buckets).unwrap();
            }
        }
    }

    builder.install().expect("failed to create metrics exporter");

    // init metric description (always after provider started)
    for metric in &metrics {
        metric.register_description();
    }
}
