use std::sync::Arc;

use alloy_rpc_types_trace::geth::GethTrace;
use anyhow::anyhow;
use anyhow::bail;
use stratus_metrics as metrics;

use crate::GlobalState;
use crate::eth::executor::ExecutionMetrics;
use crate::eth::executor::ExecutorConfig;
use crate::eth::executor::ExecutorError;
use crate::eth::executor::evm::Evm;
use crate::eth::executor::evm::EvmKind;
use crate::eth::executor::evm::RevmResultAndState;
use crate::eth::executor::evm::types::InspectorInput;
use crate::eth::executor::types::EvmRoute;
use crate::eth::executor::types::ExecutionTask;
use crate::eth::executor::types::InspectionTask;
use crate::eth::executor::types::PoolTask;
use crate::eth::storage::StratusStorage;
use crate::eth::types::StratusError;
use crate::eth::types::UnexpectedError;
use crate::ext::spawn_thread;
use crate::infra::tracing::warn_task_tx_closed;
use crate::utils::Permit;
use crate::utils::Semaphore;
use crate::utils::SemaphoreMetrics;

/// Total capacity of the unified EVM pool task queue.
const TASK_QUEUE_CAPACITY: usize = 4096;

/// Default number of EVM workers in the unified pool (sum of the old per-kind pool defaults).
const DEFAULT_WORKERS: usize = 150;

/// Default maximum number of concurrent call-past and inspector executions.
const DEFAULT_KIND_LIMIT: usize = 50;

/// Effective configuration of the unified EVM pool, resolved from [`ExecutorConfig`].
#[derive(Clone, Copy, Debug)]
pub struct PoolConfig {
    /// Total number of EVM workers, shared by every execution kind.
    pub workers: usize,

    /// Maximum number of concurrent call-present executions.
    pub call_present_limit: usize,

    /// Maximum number of concurrent call-past executions.
    pub call_past_limit: usize,

    /// Maximum number of concurrent inspector executions.
    pub inspector_limit: usize,

    /// Extra permits that any execution kind can borrow when its own limit is exhausted.
    pub flex_quota: usize,
}

impl PoolConfig {
    /// Resolves the effective pool configuration, mapping deprecated fields to their new meaning.
    pub fn resolve(config: &ExecutorConfig) -> anyhow::Result<Self> {
        if config.evm_workers == Some(0) {
            bail!("executor.evm_workers must be greater than zero");
        }

        for (field, value) in [
            ("executor.call_present_evms", config.call_present_evms),
            ("executor.call_past_evms", config.call_past_evms),
            ("executor.inspector_evms", config.inspector_evms),
        ] {
            if value.is_some() {
                tracing::warn!(
                    field,
                    "deprecated executor pool field; use executor.evm_workers and the per-kind limit fields instead"
                );
            }
        }

        let call_past_limit = config.call_past_limit.or(config.call_past_evms).unwrap_or(DEFAULT_KIND_LIMIT);
        let inspector_limit = config.inspector_limit.or(config.inspector_evms).unwrap_or(DEFAULT_KIND_LIMIT);

        let workers = match config.evm_workers {
            Some(workers) => workers,
            // no old field set: default pool size
            None if !config.has_deprecated_pool_sizes() => DEFAULT_WORKERS,
            // at least one old field set: preserve the total capacity of the old per-kind pools
            None =>
                config.call_present_evms.unwrap_or(DEFAULT_KIND_LIMIT)
                    + config.call_past_evms.unwrap_or(DEFAULT_KIND_LIMIT)
                    + config.inspector_evms.unwrap_or(DEFAULT_KIND_LIMIT),
        };

        let call_present_limit = config
            .call_present_limit
            .or(config.call_present_evms)
            .unwrap_or_else(|| workers.saturating_sub(call_past_limit + inspector_limit));

        let resolved = Self {
            workers,
            call_present_limit,
            call_past_limit,
            inspector_limit,
            flex_quota: config.evm_flex_quota,
        };

        let limits_sum = call_present_limit + call_past_limit + inspector_limit;
        if limits_sum > workers {
            bail!(
                "executor pool kind limits ({call_present_limit} call-present + {call_past_limit} call-past + {inspector_limit} inspector = {limits_sum}) \
                 exceed the total number of workers ({workers}); increase executor.evm_workers or lower the limits"
            );
        }

        tracing::info!(?resolved, "unified EVM pool configuration resolved");
        Ok(resolved)
    }
}

/// Per-kind concurrency limits of the unified EVM pool.
struct PoolLimits {
    call_present: Semaphore,
    call_past: Semaphore,
    inspector: Semaphore,
    flex: Semaphore,
}

impl PoolLimits {
    fn new(config: PoolConfig) -> Self {
        Self {
            call_present: Semaphore::with_metrics(config.call_present_limit, SemaphoreMetrics::Pool("call_present")),
            call_past: Semaphore::with_metrics(config.call_past_limit, SemaphoreMetrics::Pool("call_past")),
            inspector: Semaphore::with_metrics(config.inspector_limit, SemaphoreMetrics::Pool("inspector")),
            flex: Semaphore::with_metrics(config.flex_quota, SemaphoreMetrics::Pool("flex")),
        }
    }

    /// Own-limit semaphore of an execution kind.
    fn own(&self, kind: EvmKind) -> &Semaphore {
        match kind {
            EvmKind::CallPresent => &self.call_present,
            EvmKind::CallPast => &self.call_past,
            EvmKind::Inspect => &self.inspector,
            EvmKind::Transaction => unreachable!("transaction execution is not managed by the unified EVM pool"),
        }
    }

    /// Acquires a permit for the kind, borrowing from the flex quota when the own limit is exhausted.
    fn acquire(&self, kind: EvmKind) -> Option<Permit> {
        if let Some(permit) = self.own(kind).try_acquire() {
            return Some(permit);
        }

        if let Some(permit) = self.flex.try_acquire() {
            return Some(permit);
        }

        self.own(kind).acquire_shutdown_aware()
    }
}

/// Manages the unified EVM pool: one shared set of workers serving every execution kind.
pub struct EvmWorkerPool {
    tx: crossbeam_channel::Sender<PoolTask>,
    limits: Arc<PoolLimits>,
}

impl EvmWorkerPool {
    /// Spawns the unified EVM pool workers.
    pub fn spawn(storage: Arc<StratusStorage>, config: &ExecutorConfig) -> anyhow::Result<Self> {
        let pool = PoolConfig::resolve(config)?;
        let (tx, rx) = crossbeam_channel::bounded::<PoolTask>(TASK_QUEUE_CAPACITY);

        for worker_index in 1..=pool.workers {
            let task_name = format!("evm-pool-{worker_index}");
            let worker_storage = Arc::clone(&storage);
            let worker_config = *config;
            let worker_rx = rx.clone();
            let thread_name = task_name.clone();
            spawn_thread(&thread_name, move || {
                Self::worker(&task_name, worker_storage, worker_config, worker_rx);
            });
        }

        // initialize the gauges so every kind series exists from startup
        for kind in [EvmKind::CallPresent, EvmKind::CallPast, EvmKind::Inspect] {
            metrics::set_executor_workers_busy(0, kind);
        }
        metrics::set_executor_workers_total(pool.workers as u64);

        Ok(Self {
            tx,
            limits: Arc::new(PoolLimits::new(pool)),
        })
    }

    /// Executes a call in the specified route.
    pub fn execute<Output>(&self, route: EvmRoute) -> Result<(Output, ExecutionMetrics), StratusError>
    where
        Output: TryFrom<RevmResultAndState, Error = StratusError>,
    {
        let kind = match &route {
            EvmRoute::CallPresent(_) => EvmKind::CallPresent,
            EvmRoute::CallPast(_) => EvmKind::CallPast,
        };

        let Some(permit) = self.limits.acquire(kind) else {
            return Err(UnexpectedError::Unexpected(anyhow!("executor pool is shutting down")).into());
        };

        let (execution_tx, execution_rx) = oneshot::channel::<Result<(RevmResultAndState, ExecutionMetrics), StratusError>>();

        let task = match route {
            EvmRoute::CallPresent(input) => PoolTask::call(ExecutionTask::new(input, execution_tx), kind, permit),
            EvmRoute::CallPast(input) => PoolTask::call(ExecutionTask::new(input, execution_tx), kind, permit),
        };
        self.tx.send(task)?;
        metrics::set_executor_pool_queue_len(self.tx.len() as u64);

        match execution_rx.recv() {
            Ok(result) => {
                let (result, metrics) = result?;
                Ok((result.try_into()?, metrics))
            }
            Err(_) => Err(UnexpectedError::ChannelClosed { channel: "evm" }.into()),
        }
    }

    /// Executes a transaction inspection (debug_traceTransaction).
    pub fn inspect(&self, input: InspectorInput) -> Result<GethTrace, StratusError> {
        let Some(permit) = self.limits.acquire(EvmKind::Inspect) else {
            return Err(UnexpectedError::Unexpected(anyhow!("executor pool is shutting down")).into());
        };

        let (inspector_tx, inspector_rx) = oneshot::channel::<Result<GethTrace, StratusError>>();
        let task = PoolTask::inspect(InspectionTask::new(input, inspector_tx), permit);
        let _ = self.tx.send(task);
        match inspector_rx.recv() {
            Ok(result) => result,
            Err(_) => Err(UnexpectedError::ChannelClosed { channel: "evm" }.into()),
        }
    }

    /// Function executed by the unified EVM pool worker threads.
    fn worker(task_name: &str, storage: Arc<StratusStorage>, config: ExecutorConfig, task_rx: crossbeam_channel::Receiver<PoolTask>) {
        let mut evm = Evm::new(Arc::clone(&storage), &config, EvmKind::CallPresent);

        while let Ok(task) = task_rx.recv() {
            if GlobalState::is_shutdown_warn(task_name) {
                return;
            }

            if let Err(StratusError::Executor(ExecutorError::Panic { err: panic_err })) = task.execute(&mut evm) {
                tracing::error!(?panic_err, "executor panicked; recreating EVM");
                evm = Evm::new(Arc::clone(&storage), &config, EvmKind::CallPresent);
            }
        }
        warn_task_tx_closed(task_name);
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ExecutorConfig {
        ExecutorConfig {
            executor_chain_id: 1,
            ..Default::default()
        }
    }

    #[test]
    fn test_pool_config_resolves_defaults() {
        let config = test_config();
        let pool = PoolConfig::resolve(&config).unwrap();
        assert_eq!(pool.workers, DEFAULT_WORKERS);
        assert_eq!(pool.call_present_limit, 50);
        assert_eq!(pool.call_past_limit, 50);
        assert_eq!(pool.inspector_limit, 50);
        assert_eq!(pool.flex_quota, 0);
    }

    #[test]
    fn test_pool_config_maps_deprecated_fields() {
        let mut config = test_config();
        config.call_present_evms = Some(100);
        config.call_past_evms = Some(20);
        config.inspector_evms = Some(30);
        let pool = PoolConfig::resolve(&config).unwrap();
        // total capacity preserved: 100 + 20 + 30
        assert_eq!(pool.workers, 150);
        assert_eq!(pool.call_present_limit, 100);
        assert_eq!(pool.call_past_limit, 20);
        assert_eq!(pool.inspector_limit, 30);
    }

    #[test]
    fn test_pool_config_deprecated_fields_with_unset_kinds() {
        let mut config = test_config();
        config.call_present_evms = Some(100);
        let pool = PoolConfig::resolve(&config).unwrap();
        // unset deprecated fields keep their old defaults (50) when computing the total
        assert_eq!(pool.workers, 200);
        assert_eq!(pool.call_present_limit, 100);
        assert_eq!(pool.call_past_limit, 50);
        assert_eq!(pool.inspector_limit, 50);
    }

    #[test]
    fn test_pool_config_new_fields_take_precedence() {
        let mut config = test_config();
        config.evm_workers = Some(200);
        config.call_present_limit = Some(120);
        config.call_past_limit = Some(20);
        config.inspector_limit = Some(30);
        config.evm_flex_quota = 40;
        let pool = PoolConfig::resolve(&config).unwrap();
        assert_eq!(pool.workers, 200);
        assert_eq!(pool.call_present_limit, 120);
        assert_eq!(pool.call_past_limit, 20);
        assert_eq!(pool.inspector_limit, 30);
        assert_eq!(pool.flex_quota, 40);
    }

    #[test]
    fn test_pool_config_call_present_uses_remaining_capacity() {
        let mut config = test_config();
        config.evm_workers = Some(200);
        let pool = PoolConfig::resolve(&config).unwrap();
        assert_eq!(pool.call_present_limit, 200 - 50 - 50);
    }

    #[test]
    fn test_pool_config_rejects_limits_exceeding_workers() {
        let mut config = test_config();
        config.evm_workers = Some(100);
        config.call_present_limit = Some(60);
        config.call_past_limit = Some(50);
        config.inspector_limit = Some(50);
        assert!(PoolConfig::resolve(&config).is_err());
    }

    #[test]
    fn test_pool_config_rejects_zero_workers() {
        let mut config = test_config();
        config.evm_workers = Some(0);
        assert!(PoolConfig::resolve(&config).is_err());
    }
}
