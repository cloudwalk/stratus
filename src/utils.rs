use std::sync::Arc;
use std::time::Duration;

use derive_more::Deref;
use parking_lot::Condvar;
use parking_lot::Mutex;
use tokio::time::Instant;

use crate::GlobalState;

/// Amount of bytes in one GB (technically, GiB).
pub const GIGABYTE: usize = 1024 * 1024 * 1024;

pub struct DropTimer {
    instant: Instant,
    scope_name: &'static str,
}

impl DropTimer {
    pub fn start(scope_name: &'static str) -> Self {
        Self {
            instant: Instant::now(),
            scope_name,
        }
    }
}

impl Drop for DropTimer {
    fn drop(&mut self) {
        tracing::info!(ran_for = ?self.instant.elapsed(), "Timer: '{}' finished", self.scope_name);
    }
}

/// Interval between shutdown checks while blocked acquiring a shutdown-aware semaphore permit.
const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Deref, Default)]
pub struct Semaphore {
    #[deref]
    sem: Arc<SemaphoreInner>,
}

#[derive(Default)]
pub struct SemaphoreInner {
    permits: Mutex<usize>,
    cvar: Condvar,
    metrics: SemaphoreMetrics,
}

/// Metrics recorded by a [`Semaphore`] when its permits change.
#[derive(Clone, Copy, Default)]
pub enum SemaphoreMetrics {
    /// Do not record any metric.
    #[default]
    Disabled,

    /// Legacy metrics of the local transaction warmup semaphore (unlabeled).
    LocalTransaction,

    /// Unified executor pool permit metrics, labeled by semaphore kind.
    Pool(&'static str),
}

impl SemaphoreMetrics {
    fn waiting_added(&self) {
        match *self {
            Self::Disabled => {}
            Self::LocalTransaction => stratus_metrics::inc_executor_local_transaction_semaphore_waiting(1),
            Self::Pool(kind) => stratus_metrics::inc_executor_pool_permits_waiting(1, kind),
        }
    }

    fn waiting_removed(&self) {
        match *self {
            Self::Disabled => {}
            Self::LocalTransaction => stratus_metrics::dec_executor_local_transaction_semaphore_waiting(1),
            Self::Pool(kind) => stratus_metrics::dec_executor_pool_permits_waiting(1, kind),
        }
    }

    fn permit_acquired(&self) {
        match *self {
            Self::Disabled => {}
            Self::LocalTransaction => stratus_metrics::inc_executor_local_transaction_permit_holders(1),
            Self::Pool(kind) => stratus_metrics::inc_executor_pool_permits_held(1, kind),
        }
    }

    fn permit_released(&self) {
        match *self {
            Self::Disabled => {}
            Self::LocalTransaction => stratus_metrics::dec_executor_local_transaction_permit_holders(1),
            Self::Pool(kind) => stratus_metrics::dec_executor_pool_permits_held(1, kind),
        }
    }
}

pub struct Permit {
    sem: Arc<SemaphoreInner>,
}

impl Semaphore {
    pub fn new(permits: usize) -> Self {
        Self::with_metrics(permits, SemaphoreMetrics::Disabled)
    }

    pub fn with_metrics(permits: usize, metrics: SemaphoreMetrics) -> Self {
        Self {
            sem: Arc::new(SemaphoreInner {
                permits: Mutex::new(permits),
                cvar: Condvar::new(),
                metrics,
            }),
        }
    }

    /// Blocks until a permit is available.
    pub fn acquire(&self) -> Permit {
        self.metrics.waiting_added();
        let mut permits = self.permits.lock();
        while *permits == 0 {
            self.cvar.wait(&mut permits);
        }
        *permits -= 1;
        drop(permits);
        self.metrics.waiting_removed();
        self.metrics.permit_acquired();
        Permit { sem: Arc::clone(&self.sem) }
    }

    /// Tries to acquire a permit without blocking.
    pub fn try_acquire(&self) -> Option<Permit> {
        let mut permits = self.permits.lock();
        if *permits == 0 {
            return None;
        }
        *permits -= 1;
        drop(permits);
        self.metrics.permit_acquired();
        Some(Permit { sem: Arc::clone(&self.sem) })
    }

    /// Blocks until a permit is available or the application starts shutting down.
    pub fn acquire_shutdown_aware(&self) -> Option<Permit> {
        self.metrics.waiting_added();
        let mut permits = self.permits.lock();
        while *permits == 0 {
            if GlobalState::is_shutdown() {
                drop(permits);
                self.metrics.waiting_removed();
                return None;
            }
            self.cvar.wait_for(&mut permits, SHUTDOWN_POLL_INTERVAL);
        }
        *permits -= 1;
        drop(permits);
        self.metrics.waiting_removed();
        self.metrics.permit_acquired();
        Some(Permit { sem: Arc::clone(&self.sem) })
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        let mut permits = self.sem.permits.lock();
        *permits += 1;
        self.sem.cvar.notify_one();
        self.sem.metrics.permit_released();
    }
}

#[cfg(test)]
pub mod test_utils {
    use alloy_primitives::Uint;
    use anyhow::Context;
    use fake::Dummy;
    use fake::Fake;
    use fake::Faker;
    use glob::glob;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    fn generate_rng() -> rand::rngs::SmallRng {
        use std::time::SystemTime;
        use std::time::UNIX_EPOCH;

        use rand::SeedableRng;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).expect("Failed to get system time").as_secs();
        rand::rngs::SmallRng::seed_from_u64(now)
    }

    fn deterministic_rng() -> SmallRng {
        SeedableRng::seed_from_u64(0)
    }

    /// Fake the first `size` values of type `T` using the default seed.
    ///
    /// Multiple calls of this (for the same `T` and `size`) will return the same list.
    pub fn fake_list<T: fake::Dummy<Faker>>(size: usize) -> Vec<T> {
        let mut rng = deterministic_rng();
        (0..size).map(|_| Faker.fake_with_rng::<T, _>(&mut rng)).collect()
    }

    /// Fake the first `T` value in the default seed.
    ///
    /// Multiple calls of this (for the same `T`) will return the same value.
    pub fn fake_first<T: fake::Dummy<Faker>>() -> T {
        let mut rng = deterministic_rng();
        Faker.fake_with_rng::<T, _>(&mut rng)
    }

    pub fn glob_to_string_paths(pattern: impl AsRef<str>) -> anyhow::Result<Vec<String>> {
        let pattern = pattern.as_ref();

        let iter = glob(pattern).with_context(|| format!("failed to parse glob pattern: {pattern}"))?;
        let mut paths = vec![];

        for entry in iter {
            let entry = entry.with_context(|| format!("failed to read glob entry with pattern: {pattern}"))?;
            let path = entry.to_str().with_context(|| format!("failed to convert path to string: {entry:?}"))?;
            paths.push(path.to_owned());
        }

        Ok(paths)
    }

    pub fn fake_option<T: Dummy<Faker>>() -> Option<T> {
        let mut rng = generate_rng();
        Some(Faker.fake_with_rng::<T, _>(&mut rng))
    }

    pub fn fake_option_uint<const N: usize, const L: usize>() -> Option<Uint<N, L>> {
        let mut rng = generate_rng();
        Some(Uint::random_with(&mut rng))
    }

    pub fn fake_uint<const N: usize, const L: usize>() -> Uint<N, L> {
        let mut rng = generate_rng();
        Uint::random_with(&mut rng)
    }
}
