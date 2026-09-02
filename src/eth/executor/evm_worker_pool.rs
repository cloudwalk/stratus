use std::sync::Arc;

use alloy_rpc_types_trace::geth::GethTrace;

use crate::GlobalState;
use crate::eth::executor::EvmExecutionMetrics;
use crate::eth::executor::ExecutorConfig;
use crate::eth::executor::ExecutorError;
use crate::eth::executor::TransactionExecutionInput;
use crate::eth::executor::evm::Evm;
use crate::eth::executor::evm::EvmKind;
use crate::eth::executor::evm::JitHandle;
use crate::eth::executor::evm::RevmResultAndState;
use crate::eth::executor::evm::types::CallExecutionInput;
use crate::eth::executor::evm::types::InspectorInput;
use crate::eth::executor::types::EvmRoute;
use crate::eth::executor::types::EvmTask;
use crate::eth::executor::types::ExecutionTask;
use crate::eth::executor::types::InspectionTask;
use crate::eth::executor::types::Task;
use crate::eth::storage::StratusStorage;
use crate::eth::types::StratusError;
use crate::eth::types::UnexpectedError;
use crate::ext::spawn_thread;
use crate::infra::metrics;
use crate::infra::tracing::warn_task_tx_closed;

/// Manages EVM pool and communication channels.
pub struct EvmWorkerPool {
    /// Worker for execution of transactions.
    pub tx: crossbeam_channel::Sender<EvmTask<ExecutionTask<TransactionExecutionInput>>>,

    /// Pool for parallel execution of calls (eth_call and eth_estimateGas) reading from current state. Usually contains multiple EVMs.
    pub call_present: crossbeam_channel::Sender<EvmTask<ExecutionTask<CallExecutionInput>>>,

    /// Pool for parallel execution of calls (eth_call and eth_estimateGas) reading from past state. Usually contains multiple EVMs.
    pub call_past: crossbeam_channel::Sender<EvmTask<ExecutionTask<CallExecutionInput>>>,

    /// Pool for parallel execution of tx inspections (debug_traceTransaction). Usually contains multiple EVMs.
    pub inspector: crossbeam_channel::Sender<EvmTask<InspectionTask>>,
}

impl EvmWorkerPool {
    /// Spawns EVM tasks in background.
    pub fn spawn(storage: Arc<StratusStorage>, config: &ExecutorConfig) -> Self {
        // shared revmc JIT backend, distributed to every worker
        let jit = JitHandle::create(config);

        // function executed by evm threads
        fn worker<T: Task + Send>(
            task_name: &str,
            storage: Arc<StratusStorage>,
            config: ExecutorConfig,
            task_rx: crossbeam_channel::Receiver<EvmTask<T>>,
            kind: EvmKind,
            jit: JitHandle,
        ) {
            let mut evm = Evm::new(Arc::clone(&storage), &config, kind, jit.clone());
            let mut last_jit_refresh = std::time::Instant::now();

            // keep executing transactions until the channel is closed
            while let Ok(task) = task_rx.recv() {
                if GlobalState::is_shutdown_warn(task_name) {
                    return;
                }

                // adopt contracts compiled after their first execution: revmc pins each code
                // hash's first dispatch decision, so the dispatch cache must reset periodically
                jit.refresh_if_due(&mut evm, kind, &mut last_jit_refresh);

                let _guard = kind.mark_executor_pool_busy();
                if let Err(StratusError::Executor(ExecutorError::Panic { err: panic_err })) = task.execute(&mut evm) {
                    tracing::error!(?panic_err, "executor panicked; recreating EVM");
                    evm = Evm::new(Arc::clone(&storage), &config, kind, jit.clone());
                }
            }
            warn_task_tx_closed(task_name);
        }

        // function that spawn evm threads
        fn spawn_evms<T: Task + Send + 'static>(
            task_name: &str,
            num_evms: usize,
            kind: EvmKind,
            storage: &Arc<StratusStorage>,
            config: &ExecutorConfig,
            jit: &JitHandle,
        ) -> crossbeam_channel::Sender<EvmTask<T>> {
            let (evm_tx, evm_rx) = crossbeam_channel::unbounded::<EvmTask<T>>();

            for evm_index in 1..=num_evms {
                let evm_task_name = format!("{task_name}-{evm_index}");
                let evm_storage = Arc::clone(storage);
                let evm_config = config.clone();
                let evm_rx = evm_rx.clone();
                let evm_jit = jit.clone();
                let thread_name = evm_task_name.clone();
                spawn_thread(&thread_name, move || {
                    worker(&evm_task_name, evm_storage, evm_config, evm_rx, kind, evm_jit);
                });
            }
            metrics::set_executor_workers_busy(0, kind);
            evm_tx
        }

        let tx = spawn_evms("evm-tx", 1, EvmKind::Transaction, &storage, config, &jit);
        let call_present = spawn_evms("evm-call-present", config.call_present_evms, EvmKind::CallPresent, &storage, config, &jit);
        let call_past = spawn_evms("evm-call-past", config.call_past_evms, EvmKind::CallPast, &storage, config, &jit);
        let inspector = spawn_evms("inspector", config.inspector_evms, EvmKind::Inspect, &storage, config, &jit);

        EvmWorkerPool {
            tx,
            call_present,
            call_past,
            inspector,
        }
    }

    /// Executes a transaction in the specified route.
    pub fn execute<Output>(&self, route: EvmRoute) -> Result<(Output, EvmExecutionMetrics), StratusError>
    where
        Output: TryFrom<RevmResultAndState, Error = StratusError>,
    {
        let (execution_tx, execution_rx) = oneshot::channel::<Result<(RevmResultAndState, EvmExecutionMetrics), StratusError>>();

        match route {
            EvmRoute::Transaction(input) => {
                let task = ExecutionTask::new(input, execution_tx).into();
                self.tx.send(task)?;
            }
            EvmRoute::CallPresent(input) => {
                let task = ExecutionTask::new(input, execution_tx).into();
                self.call_present.send(task)?;
            }
            EvmRoute::CallPast(input) => {
                let task = ExecutionTask::new(input, execution_tx).into();
                self.call_past.send(task)?;
            }
        };

        match execution_rx.recv() {
            Ok(result) => {
                let (result, metrics) = result?;
                Ok((result.try_into()?, metrics))
            }
            Err(_) => Err(UnexpectedError::ChannelClosed { channel: "evm" }.into()),
        }
    }

    pub fn inspect(&self, input: InspectorInput) -> Result<GethTrace, StratusError> {
        let (inspector_tx, inspector_rx) = oneshot::channel::<Result<GethTrace, StratusError>>();
        let task = InspectionTask::new(input, inspector_tx).into();
        let _ = self.inspector.send(task);
        match inspector_rx.recv() {
            Ok(result) => result,
            Err(_) => Err(UnexpectedError::ChannelClosed { channel: "evm" }.into()),
        }
    }
}
