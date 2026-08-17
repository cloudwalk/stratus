use std::net::SocketAddr;

use clap::Parser;
use display_json::DebugAsJson;
#[cfg(feature = "metrics")]
use metrics::KeyName;
#[cfg(feature = "metrics")]
use metrics::Label;
#[cfg(feature = "metrics")]
use metrics_exporter_prometheus::PrometheusBuilder;
#[cfg(feature = "metrics")]
use metrics_tracing_context::LabelFilter;
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
    use std::thread;
    use tokio::runtime;

    let recorder = if let Ok(handle) = runtime::Handle::try_current() {
        let (recorder, exporter) = {
            let _guard = handle.enter();
            builder.build()?
        };

        handle.spawn(exporter);
        recorder
    } else {
        let runtime = runtime::Builder::new_current_thread().enable_all().build()?;

        let (recorder, exporter) = {
            let _guard = runtime.enter();
            builder.build()?
        };

        thread::Builder::new()
            .name("metrics::exporter".to_string())
            .spawn(move || runtime.block_on(exporter))?;

        recorder
    };

    let recorder = MetricsTracingContextLayer::new(StratusMetricsLabelFilter).layer(recorder);
    metrics::set_global_recorder(recorder)?;

    Ok(())
}

/// Allowlist for tracing span fields that may be injected as metric labels.
///
/// Keep this intentionally small: RPC spans contain high-cardinality fields such as ids,
/// parameters, hashes, and addresses that must never become Prometheus labels.
/// Also, please don't use TracingContextLayer::all().
#[cfg(feature = "metrics")]
#[derive(Clone, Debug)]
struct StratusMetricsLabelFilter;

#[cfg(feature = "metrics")]
impl LabelFilter for StratusMetricsLabelFilter {
    fn should_include_label(&self, name: &KeyName, label: &Label) -> bool {
        let metric_name = name.as_str();
        let label_name = label.key();

        matches!(
            (metric_name, label_name),
            (
                "stratus_executor_local_call" | "stratus_executor_local_call_account_reads" | "stratus_executor_local_call_slot_reads",
                "client" | "rpc_method" | "point_in_time"
            )
        )
    }
}

#[cfg(not(feature = "metrics"))]
fn init_metrics_exporter(_: SocketAddr) {
    tracing::info!("creating noop metrics exporter");
}

#[cfg(all(test, feature = "metrics"))]
mod tests {
    use super::*;

    #[test]
    fn test_stratus_metrics_label_filter_allows_stable_rpc_context_for_local_call_metrics() {
        let filter = StratusMetricsLabelFilter;
        let metric = KeyName::from("stratus_executor_local_call");

        assert!(filter.should_include_label(&metric, &Label::new("client", "other::test-client")));
        assert!(filter.should_include_label(&metric, &Label::new("rpc_method", "eth_call")));
        assert!(filter.should_include_label(&metric, &Label::new("point_in_time", "mined")));
    }

    #[test]
    fn test_stratus_metrics_label_filter_rejects_high_cardinality_rpc_context() {
        let filter = StratusMetricsLabelFilter;
        let metric = KeyName::from("stratus_executor_local_call");

        assert!(!filter.should_include_label(&metric, &Label::new("rpc_id", "abc123")));
        assert!(!filter.should_include_label(&metric, &Label::new("rpc_params", "[...]")));
        assert!(!filter.should_include_label(&metric, &Label::new("rpc_tx_hash", "0x123")));
    }

    #[test]
    fn test_stratus_metrics_label_filter_rejects_context_for_rpc_metrics_with_explicit_client_label() {
        let filter = StratusMetricsLabelFilter;
        let metric = KeyName::from("stratus_rpc_requests_finished");

        assert!(!filter.should_include_label(&metric, &Label::new("client", "other::test-client")));
        assert!(!filter.should_include_label(&metric, &Label::new("rpc_method", "eth_call")));
    }
}
