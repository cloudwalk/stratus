use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing::info;
#[cfg(feature = "metrics")]
use metrics;

use crate::globals::STRATUS_SHUTDOWN_SIGNAL;
use crate::infra::metrics::MetricLabelValue;

/// Type alias for label storage
type Labels = Vec<(&'static str, String)>;

#[derive(Debug, Clone)]
pub enum MetricMessage {
    /// Counter metric (increment by value)
    Counter { name: &'static str, labels: Labels, value: u64 },
    /// Histogram metric (record duration or value)
    Histogram { name: &'static str, labels: Labels, value: f64 },
    /// Gauge metric (set absolute value)
    Gauge { name: &'static str, labels: Labels, value: f64 },
}

impl MetricMessage {
    /// Create a new counter metric message
    pub fn counter(name: &'static str, labels: Labels, value: u64) -> Self {
        Self::Counter { name, labels, value }
    }

    /// Create a new histogram metric message
    pub fn histogram(name: &'static str, labels: Labels, value: f64) -> Self {
        Self::Histogram { name, labels, value }
    }

    /// Create a new gauge metric message
    pub fn gauge(name: &'static str, labels: Labels, value: f64) -> Self {
        Self::Gauge { name, labels, value }
    }

    /// Get the metric name
    pub fn name(&self) -> &'static str {
        match self {
            Self::Counter { name, .. } => name,
            Self::Histogram { name, .. } => name,
            Self::Gauge { name, .. } => name,
        }
    }

    /// Get the metric labels
    pub fn labels(&self) -> &Labels {
        match self {
            Self::Counter { labels, .. } => labels,
            Self::Histogram { labels, .. } => labels,
            Self::Gauge { labels, .. } => labels,
        }
    }
}

/// Configuration for the async metrics collector
#[derive(Debug, Clone)]
pub struct AsyncMetricsConfig {
    /// Buffer size for metrics channel
    pub buffer_size: usize,

    /// Batch size before forcing flush
    pub batch_size: usize,

    /// Maximum time before flushing batch
    pub flush_interval: Duration,
}

impl Default for AsyncMetricsConfig {
    fn default() -> Self {
        Self {
            buffer_size: 10_000,
            batch_size: 100,
            flush_interval: Duration::from_millis(50),
        }
    }
}

/// Statistics about the async metrics system
#[derive(Debug, Default, Clone)]
pub struct AsyncMetricsStats {
    // Stats struct kept for future use if needed
}

/// Non-blocking metric sender for hot paths
pub struct AsyncMetricSender {
    /// Bounded channel for sending metrics (never blocks)
    metric_tx: mpsc::Sender<MetricMessage>,

    /// Configuration
    _config: AsyncMetricsConfig,
}

impl AsyncMetricSender {
    /// Create a new async metric sender
    pub fn new(metric_tx: mpsc::Sender<MetricMessage>, config: AsyncMetricsConfig) -> Self {
        Self { metric_tx, _config: config }
    }

    pub fn try_send_metric(&self, metric: MetricMessage) {
        match self.metric_tx.try_send(metric) {
            Ok(_) => {
                // Metric sent successfully
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                // Buffer full - silently drop metric (as requested)
                // Silently drop metrics when buffer is full (no logging)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                // Channel closed - collector stopped, silently drop
            }
        }
    }
}

/// Async metrics collector that processes metrics in batches
pub struct AsyncMetricsCollector {
    /// Receiver for metric messages
    metric_rx: mpsc::Receiver<MetricMessage>,

    /// Configuration
    config: AsyncMetricsConfig,

    /// Shutdown coordination
    shutdown: CancellationToken,
}

impl AsyncMetricsCollector {
    /// Create a new async metrics collector
    pub fn new(metric_rx: mpsc::Receiver<MetricMessage>, config: AsyncMetricsConfig) -> Self {
        Self {
            metric_rx,
            config,
            shutdown: STRATUS_SHUTDOWN_SIGNAL.child_token(),
        }
    }

    /// Run the async metrics collector (main processing loop)
    pub async fn run(&mut self) {
        info!(
            buffer_size = self.config.buffer_size,
            batch_size = self.config.batch_size,
            flush_interval_ms = self.config.flush_interval.as_millis(),
            "Starting async metrics collector"
        );

        let mut batch = Vec::with_capacity(self.config.batch_size);
        let mut flush_timer = tokio::time::interval(self.config.flush_interval);

        flush_timer.tick().await;

        loop {
            tokio::select! {
                // Receive metrics (non-blocking)
                msg = self.metric_rx.recv() => {
                    match msg {
                        Some(metric) => {
                            batch.push(metric);

                            // Process batch when it reaches target size
                            if batch.len() >= self.config.batch_size {
                                Self::process_batch(&mut batch);
                            }
                        }
                        None => {
                            // Channel closed - process remaining metrics and exit
                            if !batch.is_empty() {
                                Self::process_batch(&mut batch);
                            }
                            break;
                        }
                    }
                }

                // Periodic flush timer
                _ = flush_timer.tick() => {
                    if !batch.is_empty() {
                        debug!(batch_size = batch.len(), "Flushing metrics batch on timer");
                        Self::process_batch(&mut batch);
                    }
                }

                // Graceful shutdown
                _ = self.shutdown.cancelled() => {
                    info!("Async metrics collector received shutdown signal");

                    // Process remaining metrics
                    if !batch.is_empty() {
                        info!(remaining_metrics = batch.len(), "Processing remaining metrics before shutdown");
                        Self::process_batch(&mut batch);
                    }

                    // Drain any remaining messages in the channel
                    let mut drained = 0;
                    while let Ok(metric) = self.metric_rx.try_recv() {
                        batch.push(metric);
                        drained += 1;

                        if batch.len() >= self.config.batch_size {
                            Self::process_batch(&mut batch);
                        }
                    }

                    if !batch.is_empty() {
                        Self::process_batch(&mut batch);
                    }

                    if drained > 0 {
                        info!(drained_metrics = drained, "Drained remaining metrics from channel");
                    }

                    break;
                }
            }
        }

        info!("Async metrics collector stopped");
    }

    /// Process a batch of metrics by recording them directly to the metrics registry
    fn process_batch(batch: &mut Vec<MetricMessage>) {
        if batch.is_empty() {
            return;
        }

        let batch_size = batch.len();
        debug!(batch_size, "Processing metrics batch");

        // Process each metric in the batch and record it to Prometheus
        for metric in batch.iter() {
            Self::record_metric_to_prometheus(metric);
        }

        // Clear the batch for reuse
        batch.clear();
    }

    /// Record a single metric to the Prometheus registry using the metrics crate
    #[cfg(feature = "metrics")]
    fn record_metric_to_prometheus(metric: &MetricMessage) {
        use metrics::Label;

        // Convert our labels to metrics crate format
        let labels: Vec<Label> = metric
            .labels()
            .iter()
            .map(|(key, value)| Label::new(*key, value.clone()))
            .collect();

        // Record the metric based on its type
        match metric {
            MetricMessage::Counter { name, value, .. } => {
                metrics::counter!(*name, labels.clone()).increment(*value);
            }
            MetricMessage::Histogram { name, value, .. } => {
                metrics::histogram!(*name, labels.clone()).record(*value);
            }
            MetricMessage::Gauge { name, value, .. } => {
                metrics::gauge!(*name, labels).set(*value);
            }
        }
    }

    /// No-op version when metrics feature is disabled
    #[cfg(not(feature = "metrics"))]
    fn record_metric_to_prometheus(_metric: &MetricMessage) {
        // No-op when metrics are disabled
    }
}

/// Helper function to create labels from various input types
pub fn create_labels<T: Into<MetricLabelValue>>(labels: Vec<(&'static str, T)>) -> Labels {
    let mut result = Vec::new();

    for (key, value) in labels {
        match value.into() {
            MetricLabelValue::Some(v) => result.push((key, v)),
            MetricLabelValue::None => {} // Skip None values
        }
    }

    result
}
