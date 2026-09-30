use stratus_metrics::MetricLabelValue;
use stratus_metrics::ToMetricLabelValue;
use stratus_metrics::dec_executor_workers_busy;
use stratus_metrics::inc_executor_workers_busy;

use crate::eth::executor::Lane;

// -----------------------------------------------------------------------------
// Lane metric labels
// -----------------------------------------------------------------------------

impl ToMetricLabelValue for Lane {
    fn to_metric_label_value(&self) -> MetricLabelValue {
        (*self).into()
    }
}

impl From<Lane> for MetricLabelValue {
    fn from(value: Lane) -> Self {
        let label = match value {
            Lane::Transaction => "transaction",
            Lane::CallPresent => "call_present",
            Lane::CallPast => "call_past",
            Lane::Inspector => "inspector",
        };
        Self::Some(label.to_owned())
    }
}

// -----------------------------------------------------------------------------
// Executor pool busy workers gauge
// -----------------------------------------------------------------------------

impl Lane {
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

pub struct BusyGuard(Lane);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.mark_executor_pool_free();
    }
}
