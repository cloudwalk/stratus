mod metrics_config;
mod metrics_definitions;
mod metrics_macros;
mod metrics_types;

use std::time::Instant;

pub use metrics_config::MetricsConfig;
pub use metrics_definitions::*;
pub use metrics_types::*;

use crate::eth::executor::EvmKind;

/// Track metrics execution starting instant.
pub fn now() -> Instant {
    Instant::now()
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
