mod metrics_config;
mod metrics_definitions;
mod metrics_macros;
mod metrics_types;

use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
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

/// Per-pool counters backing the `executor_workers_busy` gauge. One static per
/// pool keeps each counter addressable by name.
static EXECUTOR_WORKERS_BUSY_TRANSACTION: AtomicU64 = AtomicU64::new(0);
static EXECUTOR_WORKERS_BUSY_CALL_PRESENT: AtomicU64 = AtomicU64::new(0);
static EXECUTOR_WORKERS_BUSY_CALL_PAST: AtomicU64 = AtomicU64::new(0);
static EXECUTOR_WORKERS_BUSY_INSPECTOR: AtomicU64 = AtomicU64::new(0);

/// Returns the busy-worker counter for the given executor pool label.
fn executor_workers_busy_counter(kind: EvmKind) -> &'static AtomicU64 {
    match kind {
        EvmKind::Transaction => &EXECUTOR_WORKERS_BUSY_TRANSACTION,
        EvmKind::CallPresent => &EXECUTOR_WORKERS_BUSY_CALL_PRESENT,
        EvmKind::CallPast => &EXECUTOR_WORKERS_BUSY_CALL_PAST,
        EvmKind::Inspect => &EXECUTOR_WORKERS_BUSY_INSPECTOR,
    }
}

/// Marks a worker in the given executor pool as busy and updates the
/// `executor_workers_busy` gauge.
pub fn mark_executor_pool_busy(kind: EvmKind) {
    let busy = executor_workers_busy_counter(kind).fetch_add(1, Ordering::Relaxed) + 1;
    set_executor_workers_busy(busy, kind);
}

/// Marks a worker in the given executor pool as free and updates the
/// `executor_workers_busy` gauge.
pub fn mark_executor_pool_free(pool: EvmKind) {
    let busy = executor_workers_busy_counter(pool).fetch_sub(1, Ordering::Relaxed) - 1;
    set_executor_workers_busy(busy, pool);
}
