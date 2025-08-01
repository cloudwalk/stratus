use std::net::SocketAddr;
use std::stringify;
use std::time::Duration;

use clap::Parser;
use display_json::DebugAsJson;

use crate::infra::metrics::metrics_collector::AsyncMetricSender;
use crate::infra::metrics::metrics_collector::AsyncMetricsCollector;
use crate::infra::metrics::metrics_collector::AsyncMetricsConfig;
use crate::infra::metrics::metrics_for_consensus;
use crate::infra::metrics::metrics_for_evm;
use crate::infra::metrics::metrics_for_executor;
use crate::infra::metrics::metrics_for_importer_online;
use crate::infra::metrics::metrics_for_json_rpc;
use crate::infra::metrics::metrics_for_kafka;
use crate::infra::metrics::metrics_for_rocks;
use crate::infra::metrics::metrics_for_storage_read;
use crate::infra::metrics::metrics_for_storage_write;
use crate::infra::metrics::metrics_macros::set_async_metrics_sender;

#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct MetricsConfig {
    /// Metrics exporter binding address.
    #[arg(long = "metrics-exporter-address", env = "METRICS_EXPORTER_ADDRESS", default_value = "0.0.0.0:9000")]
    pub metrics_exporter_address: SocketAddr,

    /// Metrics buffer size (number of metrics that can be queued).
    #[arg(long = "metrics-buffer-size", env = "METRICS_BUFFER_SIZE", default_value = "10000")]
    pub buffer_size: usize,

    /// Metrics batch size (metrics processed together).
    #[arg(long = "metrics-batch-size", env = "METRICS_BATCH_SIZE", default_value = "100")]
    pub batch_size: usize,

    /// Metrics flush interval in milliseconds.
    #[arg(long = "metrics-flush-interval-ms", env = "METRICS_FLUSH_INTERVAL_MS", default_value = "50")]
    pub flush_interval_ms: u64,
}

impl MetricsConfig {
    /// Inits application global metrics exporter and async metrics system.
    pub fn init(&self) -> anyhow::Result<()> {
        tracing::info!(address = %self.metrics_exporter_address, "creating metrics exporter");

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

        // init metrics system
        let config = self.metrics_config();
        Self::init_metrics(config)?;

        Ok(())
    }

    /// Create metrics configuration from CLI/env settings
    pub fn metrics_config(&self) -> AsyncMetricsConfig {
        AsyncMetricsConfig {
            buffer_size: self.buffer_size,
            batch_size: self.batch_size,
            flush_interval: Duration::from_millis(self.flush_interval_ms),
        }
    }

    /// Initialize the metrics system
    fn init_metrics(config: AsyncMetricsConfig) -> anyhow::Result<()> {
        let (tx, rx) = tokio::sync::mpsc::channel(config.buffer_size);

        let sender = AsyncMetricSender::new(tx, config.clone());
        let mut collector = AsyncMetricsCollector::new(rx, config.clone());

        // Spawn background collector task
        tokio::spawn(async move {
            collector.run().await;
        });

        // Set global sender
        set_async_metrics_sender(sender).map_err(|_| anyhow::anyhow!("Async metrics already initialized"))?;

        tracing::info!(
            buffer_size = config.buffer_size,
            batch_size = config.batch_size,
            flush_interval_ms = config.flush_interval.as_millis(),
            "Async metrics system initialized"
        );

        Ok(())
    }
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
