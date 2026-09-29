use std::sync::Arc;

use alloy_rpc_types_trace::geth::GethTrace;
use stratus_metrics as metrics;

use crate::GlobalState;
use crate::eth::executor::ExecutionMetrics;
use crate::eth::executor::ExecutorConfig;
use crate::eth::executor::ExecutorError;
use crate::eth::executor::evm::Evm;
use crate::eth::executor::evm::EvmKind;
use crate::eth::executor::evm::RevmResultAndState;
use crate::eth::executor::evm::types::CallExecutionInput;
use crate::eth::executor::evm::types::InspectorInput;
use crate::eth::executor::pool_admission::PoolAdmission;
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

/// Manages the unified EVM pool: one shared set of workers serving every execution kind.
pub struct EvmWorkerPool {
    tx: crossbeam_channel::Sender<PoolTask>,
    admission: Arc<PoolAdmission>,
}

impl EvmWorkerPool {
    /// Spawns the unified EVM pool workers.
    pub fn spawn(storage: Arc<StratusStorage>, config: &ExecutorConfig) -> Self {
        let pool = config.pool;
        let (tx, rx) = crossbeam_channel::bounded::<PoolTask>(TASK_QUEUE_CAPACITY);
        let admission = Arc::new(PoolAdmission::new(pool));

        for worker_index in 1..=pool.evm_workers {
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
        metrics::set_executor_workers_total(pool.evm_workers as u64);

        Self { tx, admission }
    }

    /// Executes a call in the specified route.
    pub fn execute<Output>(&self, input: CallExecutionInput) -> Result<(Output, ExecutionMetrics), StratusError>
    where
        Output: TryFrom<RevmResultAndState, Error = StratusError>,
    {
        let (execution_tx, execution_rx) = oneshot::channel::<Result<(RevmResultAndState, ExecutionMetrics), StratusError>>();

        let task = PoolTask::call(ExecutionTask::new(input, execution_tx), &self.admission)?;
        self.tx.send(task)?;

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
        let (inspector_tx, inspector_rx) = oneshot::channel::<Result<GethTrace, StratusError>>();
        let task = PoolTask::inspect(InspectionTask::new(input, inspector_tx), &self.admission)?;
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
    use clap::Parser;

    use super::*;

    fn test_config(args: &[&str]) -> ExecutorConfig {
        let mut arguments = vec!["stratus", "--executor-chain-id", "1"];
        arguments.extend_from_slice(args);
        ExecutorConfig::parse_from(arguments)
    }

    #[test]
    fn test_pool_config_resolves_defaults() {
        let pool = test_config(&[]).pool;
        assert_eq!(pool.evm_workers, 150);
        assert_eq!(pool.call_present_limit(), 50);
        assert_eq!(pool.call_past_limit, 50);
        assert_eq!(pool.inspector_limit, 50);
        assert_eq!(pool.evm_busy_threshold, 80);
        assert_eq!(pool.relaxed_limit(), 120);
        assert!(pool.validate().is_empty());
    }

    #[test]
    fn test_pool_config_explicit_call_present_limit() {
        let pool = test_config(&[
            "--executor-evm-workers",
            "200",
            "--executor-call-present-limit",
            "120",
            "--executor-call-past-limit",
            "20",
            "--executor-inspector-limit",
            "30",
        ])
        .pool;
        assert_eq!(pool.evm_workers, 200);
        assert_eq!(pool.call_present_limit(), 120);
        assert_eq!(pool.call_past_limit, 20);
        assert_eq!(pool.inspector_limit, 30);
    }

    #[test]
    fn test_pool_config_call_present_uses_remaining_capacity() {
        let pool = test_config(&["--executor-evm-workers", "200"]).pool;
        assert_eq!(pool.call_present_limit(), 200 - 50 - 50);
    }

    #[test]
    fn test_pool_config_warns_on_limits_exceeding_workers() {
        let pool = test_config(&["--executor-evm-workers", "100", "--executor-call-present-limit", "60"]).pool;
        assert!(!pool.validate().is_empty());
    }

    #[test]
    fn test_pool_config_rejects_zero_workers() {
        let result = ExecutorConfig::try_parse_from(["stratus", "--executor-chain-id", "1", "--executor-evm-workers", "0"]);
        assert!(result.is_err(), "must reject zero workers");
    }

    #[test]
    fn test_pool_config_rejects_zero_limit() {
        let result = ExecutorConfig::try_parse_from(["stratus", "--executor-chain-id", "1", "--executor-call-past-limit", "0"]);
        assert!(result.is_err(), "must reject zero limit");
    }

    #[test]
    fn test_pool_config_rejects_threshold_above_100() {
        let result = ExecutorConfig::try_parse_from(["stratus", "--executor-chain-id", "1", "--executor-evm-busy-threshold", "101"]);
        assert!(result.is_err(), "must reject threshold above 100");
    }

    #[test]
    fn test_pool_config_zero_threshold_is_strict() {
        let pool = test_config(&["--executor-evm-busy-threshold", "0"]).pool;
        assert_eq!(pool.relaxed_limit(), 0);
    }
}
