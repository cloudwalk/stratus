use std::cmp::max;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::anyhow;
use tracing::info_span;
use tracing::Span;

use crate::eth::executor::Evm;
use crate::eth::executor::EvmExecutionResult;
use crate::eth::executor::EvmInput;
use crate::eth::executor::ExecutorConfig;
use crate::eth::executor::Revm;
use crate::eth::miner::Miner;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::ChainId;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::ExecutionMetrics;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalReceipts;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::ExternalTransactionExecution;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::eth::storage::StoragePointInTime;
use crate::eth::storage::StratusStorage;
use crate::ext::spawn_thread;
use crate::ext::to_json_string;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::tracing::warn_task_tx_closed;
use crate::infra::tracing::SpanExt;
use crate::GlobalState;

// -----------------------------------------------------------------------------
// Evm task
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct EvmTask {
    pub span: Span,
    pub input: EvmInput,
    pub response_tx: oneshot::Sender<Result<EvmExecutionResult, StratusError>>,
}

impl EvmTask {
    pub fn new(input: EvmInput, response_tx: oneshot::Sender<Result<EvmExecutionResult, StratusError>>) -> Self {
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

/// Manages EVM pool and communication channels.
struct Evms {
    /// Pool for parallel execution of transactions received via `eth_sendRawTransaction`. Usually contains multiple EVMs.
    pub tx_parallel: crossbeam_channel::Sender<EvmTask>,

    /// Pool for serial execution of transactions received via `eth_sendRawTransaction`. Usually contains a single EVM.
    pub tx_serial: crossbeam_channel::Sender<EvmTask>,

    /// Pool for serial execution of external transactions received via `importer-online` or `importer-offline`. Usually contains a single EVM.
    pub tx_external: crossbeam_channel::Sender<EvmTask>,

    /// Pool for parallel execution of calls (eth_call and eth_estimateGas) reading from current state. Usually contains multiple EVMs.
    pub call_present: crossbeam_channel::Sender<EvmTask>,

    /// Pool for parallel execution of calls (eth_call and eth_estimateGas) reading from past state. Usually contains multiple EVMs.
    pub call_past: crossbeam_channel::Sender<EvmTask>,
}

impl Evms {
    /// Spawns EVM tasks in background.
    fn spawn(storage: Arc<StratusStorage>, config: &ExecutorConfig) -> Self {
        let chain_id: ChainId = config.chain_id.into();

        // function executed by evm threads
        fn evm_loop(task_name: &str, storage: Arc<StratusStorage>, chain_id: ChainId, task_rx: crossbeam_channel::Receiver<EvmTask>) {
            let mut evm = Revm::new(storage, chain_id);

            // keep executing transactions until the channel is closed
            while let Ok(task) = task_rx.recv() {
                if GlobalState::is_shutdown_warn(task_name) {
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

        // function that spawn evm threads
        let spawn_evms = |task_name: &str, num_evms: usize| {
            let (evm_tx, evm_rx) = crossbeam_channel::unbounded::<EvmTask>();

            for evm_index in 1..=num_evms {
                let evm_storage = Arc::clone(&storage);
                let evm_rx = evm_rx.clone();
                let task_name = format!("{}-{}", task_name, evm_index);
                let thread_name = task_name.clone();
                spawn_thread(&thread_name, move || {
                    evm_loop(&task_name, evm_storage, chain_id, evm_rx);
                });
            }
            evm_tx
        };

        let tx_parallel = match config.strategy {
            ExecutorStrategy::Serial => spawn_evms("evm-tx-unused", 1), // should not really be used if strategy is serial, but keep 1 for fallback
            ExecutorStrategy::Paralell => spawn_evms("evm-tx-parallel", config.num_evms),
        };
        let tx_serial = spawn_evms("evm-tx-serial", 1);
        let tx_external = spawn_evms("evm-tx-external", 1);
        let call_present = spawn_evms("evm-call-present", max(config.num_evms / 2, 1));
        let call_past = spawn_evms("evm-call-past", max(config.num_evms / 4, 1));

        Evms {
            tx_parallel,
            tx_serial,
            tx_external,
            call_present,
            call_past,
        }
    }

    /// Executes a transaction in the specified route.
    fn execute(&self, evm_input: EvmInput, route: EvmRoute) -> Result<EvmExecutionResult, StratusError> {
        let (execution_tx, execution_rx) = oneshot::channel::<Result<EvmExecutionResult, StratusError>>();

        let task = EvmTask::new(evm_input, execution_tx);
        let _ = match route {
            EvmRoute::Parallel => self.tx_parallel.send(task),
            EvmRoute::Serial => self.tx_serial.send(task),
            EvmRoute::External => self.tx_external.send(task),
            EvmRoute::CallPresent => self.call_present.send(task),
            EvmRoute::CallPast => self.call_past.send(task),
        };

        match execution_rx.recv() {
            Ok(result) => result,
            Err(_) => Err(StratusError::UnexpectedChannelRead { name: "evm" }),
        }
    }
}

#[derive(Debug, Clone, Copy, strum::Display)]
pub enum EvmRoute {
    #[strum(to_string = "parallel")]
    Parallel,

    #[strum(to_string = "serial")]
    Serial,

    #[strum(to_string = "external")]
    External,

    #[strum(to_string = "call_present")]
    CallPresent,

    #[strum(to_string = "call_past")]
    CallPast,
}

// -----------------------------------------------------------------------------
// Executor
// -----------------------------------------------------------------------------

pub struct Executor {
    // Executor configuration.
    config: ExecutorConfig,

    /// Channels to send transactions to background EVMs.
    evms: Evms,

    /// Mutex-wrapped miner for creating new blockchain blocks.
    miner: Arc<Miner>,

    /// Shared storage backend for persisting blockchain state.
    storage: Arc<StratusStorage>,
}

impl Executor {
    /// Creates a new [`Executor`].
    pub fn new(storage: Arc<StratusStorage>, miner: Arc<Miner>, config: ExecutorConfig) -> Self {
        tracing::info!(?config, "creating executor");
        let evms = Evms::spawn(Arc::clone(&storage), &config);
        Self { evms, config, miner, storage }
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

        // track pending block number
        let storage = &self.storage;
        storage.set_pending_external_block(block.clone())?;
        storage.set_pending_block_number(block.number())?;

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
            let sender = self.storage.read_account(&receipt.from.into(), &StoragePointInTime::Pending)?;
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
        let evm_result = self.evms.execute(evm_input, EvmRoute::External);

        // handle re-execution result
        let mut evm_result = match evm_result {
            Ok(inner) => inner,
            Err(e) => {
                let json_tx = to_json_string(&tx);
                let json_receipt = to_json_string(&receipt);
                tracing::error!(reason = ?e, block_number = %block.number(), tx_hash = %tx.hash(), %json_tx, %json_receipt, "failed to reexecute external transaction");
                return Err(e.into());
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
        #[cfg(feature = "metrics")]
        let (start, function) = (metrics::now(), tx_input.extract_function());

        // track
        Span::with(|s| {
            s.rec_str("tx_hash", &tx_input.hash);
            s.rec_str("tx_from", &tx_input.signer);
            s.rec_opt("tx_to", &tx_input.to);
            s.rec_str("tx_nonce", &tx_input.nonce);
        });

        // execute according to the strategy
        const INFINITE_ATTEMPTS: usize = usize::MAX;

        let tx_execution = match self.config.strategy {
            ExecutorStrategy::Serial => self.execute_local_transaction_attempts(tx_input, EvmRoute::Serial, INFINITE_ATTEMPTS),
            ExecutorStrategy::Paralell => {
                let parallel_attempt = self.execute_local_transaction_attempts(tx_input.clone(), EvmRoute::Parallel, 1);
                match parallel_attempt {
                    Ok(tx_execution) => Ok(tx_execution),
                    Err(e) =>
                        if let Some(StratusError::TransactionConflict(_)) = e.downcast_ref::<StratusError>() {
                            self.execute_local_transaction_attempts(tx_input, EvmRoute::Serial, INFINITE_ATTEMPTS)
                        } else {
                            Err(e)
                        },
                }
            }
        };

        // track metrics
        match tx_execution {
            Ok(tx_execution) => {
                #[cfg(feature = "metrics")]
                {
                    metrics::inc_executor_transact(start.elapsed(), true, function.clone());
                    metrics::inc_executor_transact_gas(tx_execution.execution().gas.as_u64() as usize, true, function);
                }
                Ok(tx_execution)
            }
            Err(e) => {
                #[cfg(feature = "metrics")]
                {
                    metrics::inc_executor_transact(start.elapsed(), false, function.clone());
                }
                Err(e)
            }
        }
    }

    /// Executes a transaction until it reaches the max number of attempts.
    fn execute_local_transaction_attempts(&self, tx_input: TransactionInput, evm_route: EvmRoute, max_attempts: usize) -> anyhow::Result<TransactionExecution> {
        // validate
        if tx_input.signer.is_zero() {
            tracing::warn!("rejecting transaction from zero address");
            return Err(anyhow!("transaction sent from zero address is not allowed"));
        }

        // executes transaction until no more conflicts
        let mut attempt = 0;
        loop {
            attempt += 1;

            // track
            let _span = info_span!(
                "executor::local_transaction_attempt",
                %attempt,
                tx_hash = %tx_input.hash,
                tx_from = %tx_input.signer,
                tx_to = tracing::field::Empty,
                tx_nonce = %tx_input.nonce
            )
            .entered();
            Span::with(|s| {
                s.rec_opt("tx_to", &tx_input.to);
            });
            tracing::info!(
                %attempt,
                tx_hash = %tx_input.hash,
                tx_nonce = %tx_input.nonce,
                tx_from = ?tx_input.from,
                tx_signer = %tx_input.signer,
                tx_to = ?tx_input.to,
                tx_data_len = %tx_input.input.len(),
                tx_data = %tx_input.input,
                "executing local transaction attempt"
            );

            // execute transaction in evm (retry only in case of conflict, but do not retry on other failures)
            let pending_block_number = self.storage.read_pending_block_number()?.unwrap_or_default();
            let evm_input = EvmInput::from_eth_transaction(tx_input.clone(), pending_block_number);

            let evm_result = match self.evms.execute(evm_input, evm_route) {
                Ok(evm_result) => evm_result,
                Err(e) => return Err(e.into()),
            };

            // save execution to temporary storage
            // in case of failure, retry if conflict or abandon if unexpected error
            let tx_execution = TransactionExecution::new_local(tx_input.clone(), evm_result.clone());
            match self.miner.save_execution(tx_execution.clone()) {
                Ok(_) => {
                    return Ok(tx_execution);
                }
                Err(e) =>
                    if let StratusError::TransactionConflict(ref conflicts) = e {
                        tracing::warn!(%attempt, ?conflicts, "temporary storage conflict detected when saving execution");
                        if attempt >= max_attempts {
                            return Err(e.into());
                        }
                        continue;
                    } else {
                        return Err(e.into());
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

        // retrieve block info
        let pending_block_number = self.storage.read_pending_block_number()?.unwrap_or_default();
        let mined_block = match point_in_time {
            StoragePointInTime::MinedPast(number) => self.storage.read_block(&BlockFilter::Number(number))?,
            _ => None,
        };

        // execute
        let evm_input = EvmInput::from_eth_call(input, point_in_time, pending_block_number, mined_block)?;
        let evm_route = match point_in_time {
            StoragePointInTime::Mined | StoragePointInTime::Pending => EvmRoute::CallPresent,
            StoragePointInTime::MinedPast(_) => EvmRoute::CallPast,
        };
        let evm_result = self.evms.execute(evm_input, evm_route);

        #[cfg(feature = "metrics")]
        metrics::inc_executor_call(start.elapsed(), evm_result.is_ok(), function.clone());

        let execution = evm_result?.execution;

        #[cfg(feature = "metrics")]
        metrics::inc_executor_call_gas(execution.gas.as_u64() as usize, function.clone());

        Ok(execution)
    }
}

#[derive(Clone, Copy, serde::Serialize)]
pub enum ExecutorStrategy {
    #[serde(rename = "serial")]
    Serial,

    #[serde(rename = "parallel")]
    Paralell,
}

impl FromStr for ExecutorStrategy {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "serial" => Ok(Self::Serial),
            "par" | "parallel" => Ok(Self::Paralell),
            s => Err(anyhow!("unknown executor strategy: {}", s)),
        }
    }
}
