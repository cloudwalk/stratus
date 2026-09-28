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
use crate::eth::executor::pool_admission::PoolAdmission;
use crate::eth::executor::types::EvmRoute;
use crate::eth::executor::types::ExecutionTask;
use crate::eth::executor::types::InspectionTask;
use crate::eth::executor::types::PoolTask;
use crate::eth::storage::StratusStorage;
use crate::eth::types::StratusError;
use crate::eth::types::UnexpectedError;
use crate::ext::spawn_thread;
use crate::infra::tracing::warn_task_tx_closed;

/// Total capacity of the unified EVM pool task queue.
const TASK_QUEUE_CAPACITY: usize = 4096;

/// Default number of EVM workers in the unified pool (sum of the old per-kind pool defaults).
pub const DEFAULT_WORKERS: usize = 150;

/// Default maximum number of concurrent call-past and inspector executions.
pub const DEFAULT_KIND_LIMIT: usize = 50;

/// Default pool busy percentage above which per-kind limits are enforced.
pub const DEFAULT_BUSY_THRESHOLD: usize = 80;

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

    /// Pool busy percentage above which per-kind limits are enforced.
    pub busy_threshold: usize,
}

impl PoolConfig {
    /// Resolves the effective pool configuration.
    pub fn resolve(config: &ExecutorConfig) -> anyhow::Result<Self> {
        for (field, value) in [
            ("executor.evm_workers", config.evm_workers),
            ("executor.call_past_limit", config.call_past_limit),
            ("executor.inspector_limit", config.inspector_limit),
        ] {
            if value == 0 {
                bail!("{field} must be greater than zero");
            }
        }

        if let Some(0) = config.call_present_limit {
            bail!("executor.call_present_limit must be greater than zero");
        }

        if config.evm_busy_threshold > 100 {
            bail!("executor.evm_busy_threshold must be a percentage between 0 and 100");
        }

        // defaults to the remaining pool capacity
        let call_present_limit = config.call_present_limit.unwrap_or_else(|| {
            let remaining = config.evm_workers.saturating_sub(config.call_past_limit + config.inspector_limit);
            if remaining == 0 {
                tracing::warn!("call-present limit defaults to zero; call-present tasks will only be admitted while the pool is below the busy threshold");
            }
            remaining
        });

        let resolved = Self {
            workers: config.evm_workers,
            call_present_limit,
            call_past_limit: config.call_past_limit,
            inspector_limit: config.inspector_limit,
            busy_threshold: config.evm_busy_threshold,
        };

        let limits_sum = resolved.call_present_limit + resolved.call_past_limit + resolved.inspector_limit;
        if limits_sum > resolved.workers {
            bail!(
                "executor pool kind limits ({} call-present + {} call-past + {} inspector = {limits_sum}) \
                 exceed the total number of workers ({}); increase executor.evm_workers or lower the limits",
                resolved.call_present_limit,
                resolved.call_past_limit,
                resolved.inspector_limit,
                resolved.workers
            );
        }

        tracing::info!(?resolved, "unified EVM pool configuration resolved");
        Ok(resolved)
    }

    /// In-flight task count at which relaxed admission ends and per-kind limits are enforced.
    pub fn relaxed_limit(&self) -> usize {
        self.workers * self.busy_threshold / 100
    }
}

/// Manages the unified EVM pool: one shared set of workers serving every execution kind.
pub struct EvmWorkerPool {
    tx: crossbeam_channel::Sender<PoolTask>,
    admission: Arc<PoolAdmission>,
}

impl EvmWorkerPool {
    /// Spawns the unified EVM pool workers.
    pub fn spawn(storage: Arc<StratusStorage>, config: &ExecutorConfig) -> anyhow::Result<Self> {
        let pool = PoolConfig::resolve(config)?;
        let (tx, rx) = crossbeam_channel::bounded::<PoolTask>(TASK_QUEUE_CAPACITY);
        let admission = Arc::new(PoolAdmission::new(pool));

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

        Ok(Self { tx, admission })
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

        let Some(permit) = self.admission.acquire(kind) else {
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
        let Some(permit) = self.admission.acquire(EvmKind::Inspect) else {
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
        assert_eq!(pool.busy_threshold, DEFAULT_BUSY_THRESHOLD);
        assert_eq!(pool.relaxed_limit(), 120);
    }

    #[test]
    fn test_pool_config_explicit_call_present_limit() {
        let mut config = test_config();
        config.evm_workers = 200;
        config.call_present_limit = Some(120);
        config.call_past_limit = 20;
        config.inspector_limit = 30;
        let pool = PoolConfig::resolve(&config).unwrap();
        assert_eq!(pool.workers, 200);
        assert_eq!(pool.call_present_limit, 120);
        assert_eq!(pool.call_past_limit, 20);
        assert_eq!(pool.inspector_limit, 30);
    }

    #[test]
    fn test_pool_config_call_present_uses_remaining_capacity() {
        let mut config = test_config();
        config.evm_workers = 200;
        let pool = PoolConfig::resolve(&config).unwrap();
        assert_eq!(pool.call_present_limit, 200 - 50 - 50);
    }

    #[test]
    fn test_pool_config_rejects_limits_exceeding_workers() {
        let mut config = test_config();
        config.evm_workers = 100;
        config.call_present_limit = Some(60);
        config.call_past_limit = 50;
        config.inspector_limit = 50;
        assert!(PoolConfig::resolve(&config).is_err());
    }

    #[test]
    fn test_pool_config_rejects_zero_workers() {
        let mut config = test_config();
        config.evm_workers = 0;
        assert!(PoolConfig::resolve(&config).is_err());
    }

    #[test]
    fn test_pool_config_rejects_zero_limit() {
        let mut config = test_config();
        config.call_past_limit = 0;
        assert!(PoolConfig::resolve(&config).is_err());
    }

    #[test]
    fn test_pool_config_rejects_threshold_above_100() {
        let mut config = test_config();
        config.evm_busy_threshold = 101;
        assert!(PoolConfig::resolve(&config).is_err());
    }

    #[test]
    fn test_pool_config_zero_threshold_is_strict() {
        let mut config = test_config();
        config.evm_busy_threshold = 0;
        let pool = PoolConfig::resolve(&config).unwrap();
        assert_eq!(pool.relaxed_limit(), 0);
    }
}
