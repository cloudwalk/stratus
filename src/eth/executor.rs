use std::sync::Arc;

use anyhow::anyhow;
use tracing::field;
use tracing::info_span;
use tracing::Span;

use crate::eth::evm::revm::Revm;
use crate::eth::evm::Evm;
use crate::eth::evm::EvmConfig;
use crate::eth::evm::EvmExecutionResult;
use crate::eth::evm::EvmInput;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::ChainId;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::ExecutionMetrics;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalReceipts;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::ExternalTransactionExecution;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::eth::storage::StorageError;
use crate::eth::storage::StratusStorage;
use crate::eth::BlockMiner;
use crate::ext::spawn_blocking_named_or_thread;
use crate::ext::to_json_string;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::tracing::warn_task_tx_closed;
use crate::infra::tracing::SpanExt;
use crate::log_and_err;
use crate::GlobalState;

// -----------------------------------------------------------------------------
// Evm task
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct EvmTask {
    pub span: Span,
    pub input: EvmInput,
    pub response_tx: oneshot::Sender<anyhow::Result<EvmExecutionResult>>,
}

impl EvmTask {
    pub fn new(input: EvmInput, response_tx: oneshot::Sender<anyhow::Result<EvmExecutionResult>>) -> Self {
        Self {
            span: Span::current(),
            input,
            response_tx,
        }
    }
}

// -----------------------------------------------------------------------------
// Evm communication channels
// -----------------------------------------------------------------------------

struct EvmTx {
    /// Shared-pool of EVMs used for parallel execution of transactions. Usually contains multiple EVMs.
    pub shared: crossbeam_channel::Sender<EvmTask>,

    /// Serial-pool of EVMs used for serial execution of transactions. Usually contains a single EVM.
    pub serial: crossbeam_channel::Sender<EvmTask>,
}

impl EvmTx {
    /// Executes a transaction in the shared pool of EVMs.
    fn execute_in_shared(&self, evm_input: EvmInput) -> anyhow::Result<EvmExecutionResult> {
        let (execution_tx, execution_rx) = oneshot::channel::<anyhow::Result<EvmExecutionResult>>();
        let task = EvmTask::new(evm_input, execution_tx);
        let _ = self.shared.send(task);
        execution_rx.recv()?
    }

    /// Executes a transaction in the serial pool of EVMs.
    fn execute_in_serial(&self, evm_input: EvmInput) -> anyhow::Result<EvmExecutionResult> {
        let (execution_tx, execution_rx) = oneshot::channel::<anyhow::Result<EvmExecutionResult>>();
        let task = EvmTask::new(evm_input, execution_tx);
        let _ = self.serial.send(task);
        execution_rx.recv()?
    }
}

// -----------------------------------------------------------------------------
// Executor
// -----------------------------------------------------------------------------

pub struct Executor {
    /// Channels to send transactions to background EVMs.
    evms: EvmTx,

    // Number of running EVMs.
    #[allow(unused)]
    config: EvmConfig,

    /// Mutex-wrapped miner for creating new blockchain blocks.
    miner: Arc<BlockMiner>,

    /// Shared storage backend for persisting blockchain state.
    storage: Arc<StratusStorage>,
}

impl Executor {
    /// Creates a new [`Executor`].
    pub fn new(storage: Arc<StratusStorage>, miner: Arc<BlockMiner>, config: EvmConfig) -> Self {
        tracing::info!(?config, "creating executor");

        let evm_tx = Self::spawn_evms(Arc::clone(&storage), &config);
        Self {
            evms: evm_tx,
            config,
            miner,
            storage,
        }
    }

    /// Spawns EVM tasks in background.
    fn spawn_evms(storage: Arc<StratusStorage>, config: &EvmConfig) -> EvmTx {
        let (evm_shared_tx, evm_shared_rx) = crossbeam_channel::unbounded::<EvmTask>();
        let (evm_serial_tx, evm_serial_rx) = crossbeam_channel::unbounded::<EvmTask>(); // TODO: maybe crossbeam is not the best to be used here, but it's fine for now
        let chain_id = config.chain_id;

        fn evm_loop(task_name: &str, storage: Arc<StratusStorage>, chain_id: ChainId, task_rx: crossbeam_channel::Receiver<EvmTask>) {
            let mut evm = Revm::new(storage, chain_id);

            // keep executing transactions until the channel is closed
            while let Ok(task) = task_rx.recv() {
                if GlobalState::warn_if_shutdown(task_name) {
                    return;
                }

                // execute
                let _enter = task.span.enter();
                let result = evm.execute(task.input);
                if let Err(e) = task.response_tx.send(result) {
                    tracing::error!(reason = ?e, "failed to send evm task execution result");
                }
            }

            warn_task_tx_closed(task_name);
        }

        // spawn shared evms
        for evm_index in 1..=config.num_evms {
            let evm_storage = Arc::clone(&storage);
            let evm_rx = evm_shared_rx.clone();
            let task_name = format!("evm-shared-{}", evm_index);
            spawn_blocking_named_or_thread(&format!("executor::{}", task_name), move || evm_loop(&task_name, evm_storage, chain_id, evm_rx));
        }

        // spawn serial execution evm
        spawn_blocking_named_or_thread("executor::evm-serial", move || evm_loop("evm-serial", storage, chain_id, evm_serial_rx));

        EvmTx {
            shared: evm_shared_tx,
            serial: evm_serial_tx,
        }
    }

    // -------------------------------------------------------------------------
    // External transactions
    // -------------------------------------------------------------------------

    /// Reexecutes an external block locally and imports it to the temporary storage.
    #[tracing::instrument(name = "executor::external_block", skip_all, fields(block_number))]
    pub fn execute_external_block(&self, block: &ExternalBlock, receipts: &ExternalReceipts) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        let (start, mut block_metrics) = (metrics::now(), ExecutionMetrics::default());

        Span::with(|s| {
            s.rec_str("block_number", &block.number());
        });
        tracing::info!(block_number = %block.number(), "reexecuting external block");

        // track active block number
        let storage = &self.storage;
        storage.set_active_external_block(block.clone())?;
        storage.set_active_block_number(block.number())?;

        // determine how to execute each transaction
        for tx in &block.transactions {
            let receipt = receipts.try_get(&tx.hash())?;
            let tx_execution = self.execute_external_transaction(tx, receipt, block)?;

            // track transaction metrics
            #[cfg(feature = "metrics")]
            {
                block_metrics += tx_execution.result.metrics;
            }

            // persist state
            self.miner.save_execution(TransactionExecution::External(tx_execution))?;
        }

        // track block metrics
        #[cfg(feature = "metrics")]
        {
            metrics::inc_executor_external_block(start.elapsed());
            metrics::inc_executor_external_block_account_reads(block_metrics.account_reads);
        }

        Ok(())
    }

    /// Reexecutes an external transaction locally ensuring it produces the same output.
    ///
    /// This function wraps `reexecute_external_tx_inner` and returns back the payload
    /// to facilitate re-execution of parallel transactions that failed
    #[tracing::instrument(name = "executor::external_transaction", skip_all, fields(tx_hash))]
    fn execute_external_transaction<'a, 'b>(
        &'a self,
        tx: &'b ExternalTransaction,
        receipt: &'b ExternalReceipt,
        block: &ExternalBlock,
    ) -> anyhow::Result<ExternalTransactionExecution> {
        Span::with(|s| {
            s.rec_str("tx_hash", &tx.hash);
        });

        tracing::info!(block_number = %block.number(), tx_hash = %tx.hash(), "reexecuting external transaction");

        #[cfg(feature = "metrics")]
        let start = metrics::now();

        // when transaction externally failed, create fake transaction instead of reexecuting
        if receipt.is_failure() {
            let sender = self.storage.read_account(&receipt.from.into(), &StoragePointInTime::Present)?;
            let execution = EvmExecution::from_failed_external_transaction(sender, receipt, block)?;
            let evm_result = EvmExecutionResult {
                execution,
                metrics: ExecutionMetrics::default(),
            };
            return Ok(ExternalTransactionExecution::new(tx.clone(), receipt.clone(), evm_result));
        }

        // re-execute transaction
        let evm_input = EvmInput::from_external(tx, receipt, block)?;
        #[cfg(feature = "metrics")]
        let function = evm_input.extract_function();
        let evm_result = self.evms.execute_in_serial(evm_input);

        // handle re-execution result
        let mut evm_result = match evm_result {
            Ok(inner) => inner,
            Err(e) => {
                let json_tx = to_json_string(&tx);
                let json_receipt = to_json_string(&receipt);
                tracing::error!(reason = ?e, block_number = %block.number(), tx_hash = %tx.hash(), %json_tx, %json_receipt, "failed to reexecute external transaction");
                return Err(e);
            }
        };

        // track metrics before execution is update with receipt
        #[cfg(feature = "metrics")]
        {
            metrics::inc_executor_external_transaction_gas(evm_result.execution.gas.as_u64() as usize, function.clone());
            metrics::inc_executor_external_transaction(start.elapsed(), function);
        }

        // update execution with receipt
        evm_result.execution.apply_receipt(receipt)?;

        // ensure it matches receipt before saving
        if let Err(e) = evm_result.execution.compare_with_receipt(receipt) {
            let json_tx = to_json_string(&tx);
            let json_receipt = to_json_string(&receipt);
            let json_execution_logs = to_json_string(&evm_result.execution.logs);
            tracing::error!(reason = %"mismatch reexecuting transaction", block_number = %block.number(), tx_hash = %tx.hash(), %json_tx, %json_receipt, %json_execution_logs, "failed to reexecute external transaction");
            return Err(e);
        };

        Ok(ExternalTransactionExecution::new(tx.clone(), receipt.clone(), evm_result))
    }

    // -------------------------------------------------------------------------
    // Local transactions
    // -------------------------------------------------------------------------

    /// Executes a transaction persisting state changes.
    #[tracing::instrument(name = "executor::local_transaction", skip_all, fields(tx_hash, tx_from, tx_to, tx_nonce))]
    pub fn execute_local_transaction(&self, tx_input: TransactionInput) -> anyhow::Result<TransactionExecution> {
        // track
        Span::with(|s| {
            s.rec_str("tx_hash", &tx_input.hash);
            s.rec_str("tx_from", &tx_input.signer);
            s.rec_opt("tx_to", &tx_input.to);
            s.rec_str("tx_nonce", &tx_input.nonce);
        });
        tracing::info!(
            tx_hash = %tx_input.hash,
            tx_nonce = %tx_input.nonce,
            tx_from = ?tx_input.from,
            tx_signer = %tx_input.signer,
            tx_to = ?tx_input.to,
            tx_data_len = %tx_input.input.len(),
            tx_data = %tx_input.input,
            "executing local transaction"
        );

        // execute
        self.execute_local_transaction_with_retries(tx_input, usize::MAX)
    }

    /// Executes a transaction until it reaches the max number of attempts.
    pub fn execute_local_transaction_with_retries(&self, tx_input: TransactionInput, max_attempts: usize) -> anyhow::Result<TransactionExecution> {
        #[cfg(feature = "metrics")]
        let (start, function) = (metrics::now(), tx_input.extract_function());

        // validate
        if tx_input.signer.is_zero() {
            tracing::warn!("rejecting transaction from zero address");
            return Err(anyhow!("transaction sent from zero address is not allowed"));
        }

        // executes transaction until no more conflicts
        let mut tx_attempt = 0;
        loop {
            // track
            let _span = info_span!(
                "executor::local_transaction_attempt",
                tx_hash = field::Empty,
                tx_from = field::Empty,
                tx_to = field::Empty,
                tx_nonce = field::Empty
            )
            .entered();
            Span::with(|s| {
                s.rec_str("tx_attempt", &tx_attempt);
                s.rec_str("tx_hash", &tx_input.hash);
                s.rec_str("tx_from", &tx_input.signer);
                s.rec_opt("tx_to", &tx_input.to);
                s.rec_str("tx_nonce", &tx_input.nonce);
            });
            tracing::info!(
                %tx_attempt,
                tx_hash = %tx_input.hash,
                tx_nonce = %tx_input.nonce,
                tx_from = ?tx_input.from,
                tx_signer = %tx_input.signer,
                tx_to = ?tx_input.to,
                tx_data_len = %tx_input.input.len(),
                tx_data = %tx_input.input,
                "executing local transaction attempt"
            );

            // check attempts
            tx_attempt += 1;
            if tx_attempt > max_attempts {
                return log_and_err!("aborting local transaction execution because reached max number of attempts");
            }

            // execute transaction in evm
            let evm_input = EvmInput::from_eth_transaction(tx_input.clone());
            let evm_result = self.evms.execute_in_shared(evm_input)?;

            // save execution to temporary storage
            let tx_execution = TransactionExecution::new_local(tx_input.clone(), evm_result.clone());
            match self.miner.save_execution(tx_execution.clone()) {
                Ok(_) => {
                    #[cfg(feature = "metrics")]
                    {
                        metrics::inc_executor_transact(start.elapsed(), true, function.clone());
                        metrics::inc_executor_transact_gas(tx_execution.execution().gas.as_u64() as usize, true, function);
                    }
                    return Ok(tx_execution);
                }
                Err(e) =>
                    if let Some(StorageError::Conflict(conflicts)) = e.downcast_ref::<StorageError>() {
                        tracing::warn!(%tx_attempt, ?conflicts, "temporary storage conflict detected when saving execution");
                        tx_attempt += 1;
                        continue;
                    } else {
                        #[cfg(feature = "metrics")]
                        metrics::inc_executor_transact(start.elapsed(), false, function);
                        return Err(e);
                    },
            }
        }
    }

    /// Executes a transaction without persisting state changes.
    #[tracing::instrument(name = "executor::local_call", skip_all, fields(from, to))]
    pub fn execute_local_call(&self, input: CallInput, point_in_time: StoragePointInTime) -> anyhow::Result<EvmExecution> {
        #[cfg(feature = "metrics")]
        let (start, function) = (metrics::now(), input.extract_function());

        Span::with(|s| {
            s.rec_opt("from", &input.from);
            s.rec_opt("to", &input.to);
        });
        tracing::info!(
            from = ?input.from,
            to = ?input.to,
            data_len = input.data.len(),
            data = %input.data,
            %point_in_time,
            "executing read-only local transaction"
        );

        let evm_input = EvmInput::from_eth_call(input, point_in_time);
        let evm_result = self.evms.execute_in_shared(evm_input);

        #[cfg(feature = "metrics")]
        metrics::inc_executor_call(start.elapsed(), evm_result.is_ok(), function.clone());

        let execution = evm_result?.execution;

        #[cfg(feature = "metrics")]
        metrics::inc_executor_call_gas(execution.gas.as_u64() as usize, function.clone());

        Ok(execution)
    }

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------
}
