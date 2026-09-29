use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use parking_lot::Condvar;
use parking_lot::Mutex;
use stratus_metrics as metrics;

use crate::GlobalState;
use crate::eth::executor::config::PoolConfig;
use crate::eth::executor::evm::EvmKind;
use crate::eth::types::StateError;

/// Interval between shutdown checks while blocked waiting for a kind slot.
const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Per-kind admission state, counting in-flight tasks admitted by both the relaxed and throttled paths.
struct KindGate {
    kind: EvmKind,
    limit: usize,
    inflight: Mutex<usize>,
    cvar: Condvar,
}

impl KindGate {
    fn new(kind: EvmKind, limit: usize) -> Self {
        // initialize the gauge so the kind series exists from startup
        metrics::set_executor_pool_inflight(0, kind);
        Self {
            kind,
            limit,
            inflight: Mutex::new(0),
            cvar: Condvar::new(),
        }
    }

    /// Relaxed admission: increments the in-flight count without checking the limit.
    fn admit_relaxed(&self) {
        *self.inflight.lock() += 1;
        metrics::inc_executor_pool_inflight(1, self.kind);
    }

    /// Throttled admission: blocks until the kind's in-flight count drops below its limit.
    /// Returns an error when the application starts shutting down.
    fn admit_throttled(&self) -> Result<(), StateError> {
        metrics::inc_executor_pool_waiting(1, self.kind);

        let mut inflight = self.inflight.lock();
        while *inflight >= self.limit {
            if GlobalState::is_shutdown() {
                drop(inflight);
                metrics::dec_executor_pool_waiting(1, self.kind);
                return Err(StateError::StratusShutdown);
            }
            self.cvar.wait_for(&mut inflight, SHUTDOWN_POLL_INTERVAL);
        }

        *inflight += 1;
        drop(inflight);
        metrics::dec_executor_pool_waiting(1, self.kind);
        metrics::inc_executor_pool_inflight(1, self.kind);
        Ok(())
    }

    /// Releases an admission slot of the kind.
    fn release(&self) {
        *self.inflight.lock() -= 1;
        self.cvar.notify_one();
        metrics::dec_executor_pool_inflight(1, self.kind);
    }
}

/// Admission control of the unified EVM pool: tasks of any kind are admitted immediately while the
/// pool is below the busy threshold, and limited per kind above it.
///
/// Both paths are counted by the same per-kind in-flight counters, so when the busy threshold is
/// crossed and throttling starts, executions that were admitted without a limit check are already
/// accounted for.
pub struct PoolAdmission {
    call_present: Arc<KindGate>,
    call_past: Arc<KindGate>,
    inspector: Arc<KindGate>,
    inflight_total: AtomicUsize,
    relaxed_limit: usize,
}

impl PoolAdmission {
    pub fn new(config: PoolConfig) -> Self {
        Self {
            call_present: Arc::new(KindGate::new(EvmKind::CallPresent, config.call_present_limit())),
            call_past: Arc::new(KindGate::new(EvmKind::CallPast, config.call_past_limit)),
            inspector: Arc::new(KindGate::new(EvmKind::Inspect, config.inspector_limit)),
            inflight_total: AtomicUsize::new(0),
            relaxed_limit: config.relaxed_limit(),
        }
    }

    fn gate(&self, kind: EvmKind) -> &Arc<KindGate> {
        match kind {
            EvmKind::CallPresent => &self.call_present,
            EvmKind::CallPast => &self.call_past,
            EvmKind::Inspect => &self.inspector,
            EvmKind::Transaction => unreachable!("transaction execution is not managed by the unified EVM pool"),
        }
    }

    /// Admits a task of the kind: immediately while the pool is below the busy threshold (even above
    /// the kind's limit), blocking on the kind's limit otherwise. Returns an error on shutdown.
    pub fn acquire(self: &Arc<Self>, kind: EvmKind) -> Result<PoolPermit, StateError> {
        if self.inflight_total.load(Ordering::Relaxed) < self.relaxed_limit {
            metrics::inc_executor_pool_relaxed_admissions(kind);
            self.gate(kind).admit_relaxed();
        } else {
            self.gate(kind).admit_throttled()?;
        }
        self.inflight_total.fetch_add(1, Ordering::Relaxed);
        Ok(PoolPermit {
            admission: Arc::clone(self),
            kind,
        })
    }

    /// Releases a slot taken by [`PoolAdmission::acquire`].
    fn release(&self, kind: EvmKind) {
        self.gate(kind).release();
        self.inflight_total.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Admission slot of a task in the unified EVM pool, released on drop.
pub struct PoolPermit {
    admission: Arc<PoolAdmission>,
    kind: EvmKind,
}

impl PoolPermit {
    /// Kind of the task holding the permit.
    pub(crate) fn evm_kind(&self) -> EvmKind {
        self.kind
    }
}

impl Drop for PoolPermit {
    fn drop(&mut self) {
        self.admission.release(self.kind);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    fn admission(workers: usize, busy_threshold: usize, call_past_limit: usize) -> Arc<PoolAdmission> {
        let config = PoolConfig {
            evm_workers: workers,
            call_present_limit: None,
            call_past_limit,
            inspector_limit: workers,
            evm_busy_threshold: busy_threshold,
        };
        Arc::new(PoolAdmission::new(config))
    }

    /// Spawns a thread acquiring a permit of the kind; returns a channel the thread signals on completion.
    fn spawn_acquire(admission: &Arc<PoolAdmission>, kind: EvmKind) -> mpsc::Receiver<()> {
        let (tx, rx) = mpsc::channel();
        let admission = Arc::clone(admission);
        std::thread::spawn(move || {
            let _permit = admission.acquire(kind);
            let _ = tx.send(());
        });
        rx
    }

    #[test]
    fn test_relaxed_admission_above_kind_limit() {
        // busy threshold of 80% of 10 workers: any kind is admitted while total in-flight < 8
        let admission = admission(10, 80, 1);
        let permits: Vec<_> = (0..5).map(|_| admission.acquire(EvmKind::CallPast).unwrap()).collect();
        assert_eq!(permits.len(), 5);
    }

    #[test]
    fn test_throttled_admission_blocks_at_kind_limit() {
        // threshold 80% of 1 worker: relaxed limit 0, so every admission checks the kind limit
        let admission = admission(1, 80, 1);

        let first = admission.acquire(EvmKind::CallPast).unwrap();

        let rx = spawn_acquire(&admission, EvmKind::CallPast);
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err(), "must block while the limit is held");

        drop(first);
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok(), "must proceed after the permit is released");
    }

    #[test]
    fn test_relaxed_admissions_count_toward_kind_limit() {
        // bypass executions must already be accounted for when throttling starts
        let admission = admission(10, 80, 2);

        // 3 call-past tasks bypass the limit of 2 while total in-flight < 8
        let mut permits: Vec<_> = (0..3).map(|_| admission.acquire(EvmKind::CallPast).unwrap()).collect();

        // saturate the pool to the relaxed limit with inspector tasks
        let saturating: Vec<_> = (0..5).map(|_| admission.acquire(EvmKind::Inspect).unwrap()).collect();
        assert_eq!(permits.len() + saturating.len(), 8);

        // new call-past tasks are throttled and the kind is already over its limit
        let rx = spawn_acquire(&admission, EvmKind::CallPast);
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err(), "must block while over the limit");

        // still over the limit after releasing one bypass permit
        drop(permits.swap_remove(0));
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err(), "must still block while over the limit");

        // below the limit after releasing a second one
        drop(permits.swap_remove(0));
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok(), "must proceed below the limit");
    }

    /// Ignored by default: `GlobalState` shutdown is process-global and cannot be reset, so this
    /// test would poison the other admission tests when run in parallel. Run with `--ignored`.
    #[test]
    #[ignore = "triggers process-global shutdown"]
    fn test_throttled_admission_fails_on_shutdown() {
        let admission = admission(10, 80, 1);

        let _first = admission.acquire(EvmKind::CallPast).unwrap();
        let saturating: Vec<_> = (0..7).map(|_| admission.acquire(EvmKind::Inspect).unwrap()).collect();

        let rx = spawn_acquire(&admission, EvmKind::CallPast);
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err(), "must block while over the limit");

        GlobalState::shutdown_from("test", "test_throttled_admission_returns_none_on_shutdown");
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok(), "must return on shutdown");
        drop(saturating);
    }
}
