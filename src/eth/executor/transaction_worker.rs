use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use stratus_metrics::timed;
use tracing::Span;

use crate::GlobalState;
#[cfg(feature = "metrics")]
use crate::eth::codegen;
use crate::eth::executor::EvmKind;
use crate::eth::executor::ExecutionMetrics;
use crate::eth::executor::ExecutionResult;
use crate::eth::executor::Executor;
use crate::eth::executor::ExecutorConfig;
use crate::eth::executor::ExecutorError;
use crate::eth::executor::TransactionExecution;
use crate::eth::executor::TransactionExecutionInput;
use crate::eth::executor::TransactionExecutionOutput;
use crate::eth::executor::evm::Evm;
use crate::eth::miner::Miner;
use crate::eth::storage::StorageError;
use crate::eth::storage::StratusStorage;
use crate::eth::types::BlockNumber;
use crate::eth::types::ExternalReceipt;
use crate::eth::types::ExternalTransaction;
use crate::eth::types::StratusError;
use crate::eth::types::TransactionInput;
use crate::eth::types::UnexpectedError;
use crate::ext::spawn_thread;
use crate::infra::tracing::warn_task_tx_closed;

const TASK_NAME: &str = "evm-tx-1";

type ExternalTransactionResult = anyhow::Result<()>;
type LocalTransactionResult = Result<ExecutionMetrics, StratusError>;
type LocalTransactionResponse = (Duration, LocalTransactionResult);

/// Serial worker that owns the transaction EVM and executes transaction tasks.
pub struct TransactionWorker {
    task_tx: crossbeam_channel::Sender<TransactionTask>,
}

impl TransactionWorker {
    pub fn spawn(storage: Arc<StratusStorage>, miner: Arc<Miner>, config: &ExecutorConfig) -> Self {
        let (task_tx, task_rx) = crossbeam_channel::bounded::<TransactionTask>(4096);
        let config = config.clone();

        spawn_thread(TASK_NAME, move || {
            let mut evm = Evm::new(Arc::clone(&storage), &config, EvmKind::Transaction);

            while let Ok(task) = task_rx.recv() {
                if GlobalState::is_shutdown_warn(TASK_NAME) {
                    return;
                }

                if let Err(StratusError::Executor(ExecutorError::Panic { err: panic_err })) = task.execute(&storage, &miner, &mut evm) {
                    tracing::error!(?panic_err, "executor panicked; recreating EVM");
                    evm = Evm::new(Arc::clone(&storage), &config, EvmKind::Transaction);
                }
            }
            warn_task_tx_closed(TASK_NAME);
        });

        Self { task_tx }
    }

    /// Reexecutes and persists an external transaction.
    pub fn execute_external_transaction(&self, tx: ExternalTransaction, receipt: ExternalReceipt, block_number: BlockNumber) -> ExternalTransactionResult {
        let (response_tx, response_rx) = oneshot::channel();
        self.task_tx
            .send(TransactionTask::external(tx, receipt, block_number, response_tx))
            .map_err(StratusError::from)?;
        match response_rx.recv() {
            Ok(result) => result,
            Err(_) => Err(StratusError::from(UnexpectedError::ChannelClosed { channel: "evm" }).into()),
        }
    }

    /// Executes and persists a local transaction, retrying state conflicts.
    #[timed(executor_local_transaction, labels(
        success = result.is_ok(),
        contract = |tx_input| codegen::contract_name(&tx_input.execution_info.to),
        function = |tx_input| codegen::function_sig(&tx_input.execution_info.input)
        )
    )]
    pub fn execute_local_transaction(&self, tx_input: TransactionInput) -> LocalTransactionResult {
        let (response_tx, response_rx) = oneshot::channel();
        self.task_tx.send(TransactionTask::local(tx_input, response_tx))?;
        let Ok((execution_duration, result)) = response_rx.recv() else {
            return Err(UnexpectedError::ChannelClosed { channel: "evm" }.into());
        };

        stratus_metrics::timed_duration!(execution_duration);
        result
    }

    /// Executes a transaction until it reaches the max number of attempts.
    fn execute_local_transaction_attempts(
        storage: &StratusStorage,
        miner: &Miner,
        evm: &mut Evm<TransactionExecutionInput>,
        tx_input: TransactionInput,
        max_attempts: usize,
    ) -> LocalTransactionResult {
        let mut attempt = 0;
        loop {
            attempt += 1;

            let pending_header = storage.read_pending_block_header();
            let evm_input = TransactionExecutionInput::create(&tx_input, pending_header);

            let (evm_result, evm_metrics) = evm.execute(evm_input.clone())?;
            let evm_result = TransactionExecutionOutput::try_from(evm_result)?;

            let tx_execution = TransactionExecution::new(tx_input.transaction_info, tx_input.signature, evm_input, evm_result.outcome);

            if let ExecutionResult::Reverted { reason } = &tx_execution.output.result {
                tracing::info!(?reason, "local transaction execution reverted");
                #[cfg(feature = "metrics")]
                {
                    let contract = codegen::contract_name(&tx_input.execution_info.to);
                    let function = codegen::function_sig(&tx_input.execution_info.input);
                    stratus_metrics::inc_executor_local_transaction_reverts(contract, function, reason.0.as_ref());
                }
            }

            match miner.save_execution(tx_execution, evm_result.state) {
                Ok(_) => return Ok(evm_metrics),
                Err(e) => match e {
                    StratusError::Storage(StorageError::EvmInputMismatch { ref expected, ref actual }) => {
                        tracing::warn!(?expected, ?actual, "evm input and block header mismatch");
                        if attempt >= max_attempts {
                            return Err(e);
                        }
                        continue;
                    }
                    _ => return Err(e),
                },
            }
        }
    }
}

struct TransactionTask {
    span: Span,
    kind: TransactionTaskKind,
}

impl TransactionTask {
    fn external(tx: ExternalTransaction, receipt: ExternalReceipt, block_number: BlockNumber, response_tx: oneshot::Sender<ExternalTransactionResult>) -> Self {
        Self {
            span: Span::current(),
            kind: TransactionTaskKind::External {
                tx: Box::new(tx),
                receipt: Box::new(receipt),
                block_number,
                response_tx,
            },
        }
    }

    fn local(tx_input: TransactionInput, response_tx: oneshot::Sender<LocalTransactionResponse>) -> Self {
        Self {
            span: Span::current(),
            kind: TransactionTaskKind::Local {
                tx_input: Box::new(tx_input),
                response_tx,
            },
        }
    }

    fn execute(self, storage: &StratusStorage, miner: &Miner, evm: &mut Evm<TransactionExecutionInput>) -> anyhow::Result<(), StratusError> {
        let Self { span, kind } = self;
        let _enter = span.enter();

        catch_unwind(AssertUnwindSafe(|| match kind {
            TransactionTaskKind::External {
                tx,
                receipt,
                block_number,
                response_tx,
            } => {
                let result = Executor::execute_external_transaction_inner(storage, miner, evm, *tx, *receipt, block_number);
                if let Err(e) = response_tx.send(result) {
                    tracing::error!(reason = ?e, "failed to send external transaction execution result");
                }
            }
            TransactionTaskKind::Local { tx_input, response_tx } => {
                let start = stratus_metrics::now();
                let result = TransactionWorker::execute_local_transaction_attempts(storage, miner, evm, *tx_input, usize::MAX);
                let response = (start.elapsed(), result);
                if let Err(e) = response_tx.send(response) {
                    tracing::error!(reason = ?e, "failed to send local transaction execution result");
                }
            }
        }))
        .map_err(|err| ExecutorError::Panic { err: anyhow!("{err:?}") }.into())
    }
}

enum TransactionTaskKind {
    External {
        tx: Box<ExternalTransaction>,
        receipt: Box<ExternalReceipt>,
        block_number: BlockNumber,
        response_tx: oneshot::Sender<ExternalTransactionResult>,
    },
    Local {
        tx_input: Box<TransactionInput>,
        response_tx: oneshot::Sender<LocalTransactionResponse>,
    },
}
