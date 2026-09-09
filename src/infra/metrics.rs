//! Stratus-specific metrics glue: `EvmKind` labels and the executor pool
//! busy-workers gauge. The generic metrics infrastructure, including the
//! metrics exporter configuration, lives in the `stratus_metrics` crate.

use stratus_metrics::MetricLabelValue;
use stratus_metrics::ToMetricLabelValue;
use stratus_metrics::dec_executor_workers_busy;
use stratus_metrics::inc_executor_workers_busy;

use crate::eth::executor::EvmKind;

// -----------------------------------------------------------------------------
// EvmKind metric labels
// -----------------------------------------------------------------------------

impl ToMetricLabelValue for EvmKind {
    fn to_metric_label_value(&self) -> MetricLabelValue {
        (*self).into()
    }
}

impl From<EvmKind> for MetricLabelValue {
    fn from(value: EvmKind) -> Self {
        let label = match value {
            EvmKind::Transaction => "transaction",
            EvmKind::CallPresent => "call_present",
            EvmKind::CallPast => "call_past",
            EvmKind::Inspect => "inspector",
        };
        Self::Some(label.to_owned())
    }
}

// -----------------------------------------------------------------------------
// Executor pool busy workers gauge
// -----------------------------------------------------------------------------

impl EvmKind {
    /// Marks a worker in the given executor pool as busy by atomically incrementing the `executor_workers_busy` gauge.
    /// Returns a guard that atomically decrements the gauge when dropped.
    pub fn mark_executor_pool_busy(&self) -> BusyGuard {
        inc_executor_workers_busy(1, *self);
        BusyGuard(*self)
    }

    /// Marks a worker in the given executor pool as free by atomically decrementing the `executor_workers_busy` gauge.
    fn mark_executor_pool_free(&self) {
        dec_executor_workers_busy(1, *self);
    }
}

pub struct BusyGuard(EvmKind);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.mark_executor_pool_free();
    }
}
