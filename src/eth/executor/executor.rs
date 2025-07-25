use std::cmp::max;
use std::mem;
use std::str::FromStr;
use std::sync::Arc;

#[cfg(feature = "metrics")]
use alloy_consensus::Transaction;
use alloy_rpc_types_trace::geth::GethDebugTracingOptions;
use alloy_rpc_types_trace::geth::GethTrace;
use anyhow::anyhow;
use cfg_if::cfg_if;
use parking_lot::Mutex;
use tracing::Span;
use tracing::debug_span;
#[cfg(feature = "tracing")]
use tracing::info_span;

use super::evm_input::InspectorInput;
use crate::GlobalState;
#[cfg(feature = "metrics")]
use crate::eth::codegen;
use crate::eth::executor::Evm;
use crate::eth::executor::EvmExecutionResult;
use crate::eth::executor::EvmInput;
use crate::eth::executor::ExecutorConfig;
use crate::eth::executor::evm::EvmKind;
use crate::eth::miner::Miner;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::EvmExecutionMetrics;
use crate::eth::primitives::ExecutionResult;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalReceipts;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::Hash;
use crate::eth::primitives::PointInTime;
use crate::eth::primitives::RpcError;
use crate::eth::primitives::StorageError;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionError;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::UnexpectedError;
use crate::eth::primitives::UnixTime;
use crate::eth::storage::ReadKind;
use crate::eth::storage::StratusStorage;
#[cfg(feature = "metrics")]
use crate::ext::OptionExt;
use crate::ext::spawn_thread;
use crate::ext::to_json_string;
use crate::infra::metrics;
use crate::infra::metrics::timed;
use crate::infra::tracing::SpanExt;
use crate::infra::tracing::warn_task_tx_closed;

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

pub struct InspectorTask {
    pub span: Span,
    pub input: InspectorInput,
    pub response_tx: oneshot::Sender<Result<GethTrace, StratusError>>,
}

impl InspectorTask {
    pub fn new(input: InspectorInput, response_tx: oneshot::Sender<Result<GethTrace, StratusError>>) -> Self {
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

    pub inspector: crossbeam_channel::Sender<InspectorTask>,
}

impl Evms {
    /// Spawns EVM tasks in background.
    fn spawn(storage: Arc<StratusStorage>, config: &ExecutorConfig) -> Self {
        // function executed by evm threads
        fn evm_loop(task_name: &str, storage: Arc<StratusStorage>, config: ExecutorConfig, task_rx: crossbeam_channel::Receiver<EvmTask>, kind: EvmKind) {
            let mut evm = Evm::new(storage, config, kind);

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
        let spawn_evms = |task_name: &str, num_evms: usize, kind: EvmKind| {
            let (evm_tx, evm_rx) = crossbeam_channel::unbounded::<EvmTask>();

            for evm_index in 1..=num_evms {
                let evm_task_name = format!("{task_name}-{evm_index}");
                let evm_storage = Arc::clone(&storage);
                let evm_config = config.clone();
                let evm_rx = evm_rx.clone();
                let thread_name = evm_task_name.clone();
                spawn_thread(&thread_name, move || {
                    evm_loop(&evm_task_name, evm_storage, evm_config, evm_rx, kind);
                });
            }
            evm_tx
        };

        fn inspector_loop(task_name: &str, storage: Arc<StratusStorage>, config: ExecutorConfig, task_rx: crossbeam_channel::Receiver<InspectorTask>) {
            let mut evm = Evm::new(storage, config, EvmKind::Call);

            // keep executing transactions until the channel is closed
            while let Ok(task) = task_rx.recv() {
                if GlobalState::is_shutdown_warn(task_name) {
                    return;
                }

                // execute
                let _enter = task.span.enter();
                let result = evm.inspect(task.input);
                if let Err(e) = task.response_tx.send(result) {
                    tracing::error!(reason = ?e, "failed to send evm task execution result");
                }
            }
            warn_task_tx_closed(task_name);
        }

        // function that spawn inspector threads
        let spawn_inspectors = |task_name: &str, num_evms: usize| {
            let (tx, rx) = crossbeam_channel::unbounded::<InspectorTask>();

            for index in 1..=num_evms {
                let task_name = format!("{task_name}-{index}");
                let storage = Arc::clone(&storage);
                let config = config.clone();
                let rx = rx.clone();
                let thread_name = task_name.clone();
                spawn_thread(&thread_name, move || {
                    inspector_loop(&task_name, storage, config, rx);
                });
            }
            tx
        };

        let tx_parallel = match config.executor_strategy {
            ExecutorStrategy::Serial => spawn_evms("evm-tx-unused", 1, EvmKind::Transaction), // should not really be used if strategy is serial, but keep 1 for fallback
            ExecutorStrategy::Paralell => spawn_evms("evm-tx-parallel", config.executor_evms, EvmKind::Transaction),
        };
        let tx_serial = spawn_evms("evm-tx-serial", 1, EvmKind::Transaction);
        let tx_external = spawn_evms("evm-tx-external", 1, EvmKind::Transaction);
        let call_present = spawn_evms(
            "evm-call-present",
            max(config.executor_call_present_evms.unwrap_or(config.executor_evms / 2), 1),
            EvmKind::Call,
        );
        let call_past = spawn_evms(
            "evm-call-past",
            max(config.executor_call_past_evms.unwrap_or(config.executor_evms / 4), 1),
            EvmKind::Call,
        );
        let inspector = spawn_inspectors("inspector", max(config.executor_inspector_evms.unwrap_or(config.executor_evms / 4), 1));

        Evms {
            tx_parallel,
            tx_serial,
            tx_external,
            call_present,
            call_past,
            inspector,
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
            Err(_) => Err(UnexpectedError::ChannelClosed { channel: "evm" }.into()),
        }
    }

    fn inspect(&self, input: InspectorInput) -> Result<GethTrace, StratusError> {
        let (inspector_tx, inspector_rx) = oneshot::channel::<Result<GethTrace, StratusError>>();
        let task = InspectorTask::new(input, inspector_tx);
        let _ = self.inspector.send(task);
        match inspector_rx.recv() {
            Ok(result) => result,
            Err(_) => Err(UnexpectedError::ChannelClosed { channel: "evm" }.into()),
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

/// Locks used for local execution.
#[derive(Default)]
pub struct ExecutorLocks {
    serial: Mutex<()>,
}

pub struct Executor {
    /// Executor inner locks.
    locks: ExecutorLocks,

    /// Executor configuration.
    config: ExecutorConfig,

    /// Channels to send transactions to background EVMs.
    evms: Evms,

    /// Mutex-wrapped miner for creating new blockchain blocks.
    miner: Arc<Miner>,

    /// Shared storage backend for persisting blockchain state.
    storage: Arc<StratusStorage>,
}

impl Executor {
    pub fn new(storage: Arc<StratusStorage>, miner: Arc<Miner>, config: ExecutorConfig) -> Self {
        tracing::info!(?config, "creating executor");
        let evms = Evms::spawn(Arc::clone(&storage), &config);
        Self {
            locks: ExecutorLocks::default(),
            config,
            evms,
            miner,
            storage,
        }
    }

    // -------------------------------------------------------------------------
    // External transactions
    // -------------------------------------------------------------------------

    /// Reexecutes an external block locally and imports it to the temporary storage.
    ///
    /// Returns the remaining receipts that were not consumed by the execution.
    pub fn execute_external_block(&self, mut block: ExternalBlock, mut receipts: ExternalReceipts) -> anyhow::Result<()> {
        // track
        #[cfg(feature = "metrics")]
        let (start, mut block_metrics) = (metrics::now(), EvmExecutionMetrics::default());

        #[cfg(feature = "tracing")]
        let _span = info_span!("executor::external_block", block_number = %block.number()).entered();
        tracing::info!(block_number = %block.number(), "reexecuting external block");

        // track pending block
        let block_number = block.number();
        let block_timestamp = block.timestamp();
        let block_transactions = mem::take(&mut block.transactions);

        // determine how to execute each transaction
        for tx in block_transactions.into_transactions() {
            let receipt = receipts.try_remove(tx.hash())?;
            self.execute_external_transaction(
                tx,
                receipt,
                block_number,
                block_timestamp,
                #[cfg(feature = "metrics")]
                &mut block_metrics,
            )?;
        }

        // track block metrics
        #[cfg(feature = "metrics")]
        {
            metrics::inc_executor_external_block(start.elapsed());
            metrics::inc_executor_external_block_account_reads(block_metrics.account_reads);
            metrics::inc_executor_external_block_slot_reads(block_metrics.slot_reads);
        }

        Ok(())
    }

    /// Reexecutes an external transaction locally ensuring it produces the same output.
    ///
    /// This function wraps `reexecute_external_tx_inner` and returns back the payload
    /// to facilitate re-execution of parallel transactions that failed
    fn execute_external_transaction(
        &self,
        tx: ExternalTransaction,
        receipt: ExternalReceipt,
        block_number: BlockNumber,
        block_timestamp: UnixTime,
        #[cfg(feature = "metrics")] block_metrics: &mut EvmExecutionMetrics,
    ) -> anyhow::Result<()> {
        // track
        #[cfg(feature = "metrics")]
        let (start, tx_function, tx_contract) = (
            metrics::now(),
            codegen::function_sig(tx.inner.input()),
            codegen::contract_name(&tx.0.to().map_into()),
        );

        #[cfg(feature = "tracing")]
        let _span = info_span!("executor::external_transaction", tx_hash = %tx.hash()).entered();
        tracing::info!(%block_number, tx_hash = %tx.hash(), "reexecuting external transaction");

        let evm_input = EvmInput::from_external(&tx, &receipt, block_number, block_timestamp)?;

        // when transaction externally failed, create fake transaction instead of reexecuting
        let tx_execution = match receipt.is_success() {
            // successful external transaction, re-execute locally
            true => {
                // re-execute transaction
                let evm_execution = self.evms.execute(evm_input.clone(), EvmRoute::External);

                // handle re-execution result
                let mut evm_execution = match evm_execution {
                    Ok(inner) => inner,
                    Err(e) => {
                        let json_tx = to_json_string(&tx);
                        let json_receipt = to_json_string(&receipt);
                        tracing::error!(reason = ?e, %block_number, tx_hash = %tx.hash(), %json_tx, %json_receipt, "failed to reexecute external transaction");
                        return Err(e.into());
                    }
                };

                // update execution with receipt
                evm_execution.execution.apply_receipt(&receipt)?;

                // ensure it matches receipt before saving
                if let Err(e) = evm_execution.execution.compare_with_receipt(&receipt) {
                    let json_tx = to_json_string(&tx);
                    let json_receipt = to_json_string(&receipt);
                    let json_execution_logs = to_json_string(&evm_execution.execution.logs);
                    tracing::error!(reason = ?e, %block_number, tx_hash = %tx.hash(), %json_tx, %json_receipt, %json_execution_logs, "failed to reexecute external transaction");
                    return Err(e);
                };

                TransactionExecution::new(tx.try_into()?, evm_input, evm_execution)
            }
            //
            // failed external transaction, re-create from receipt without re-executing
            false => {
                let sender = self.storage.read_account(receipt.from.into(), PointInTime::Pending, ReadKind::Transaction)?;
                let execution = EvmExecution::from_failed_external_transaction(sender, &receipt, block_timestamp)?;
                let evm_result = EvmExecutionResult {
                    execution,
                    metrics: EvmExecutionMetrics::default(),
                };
                TransactionExecution::new(tx.try_into()?, evm_input, evm_result)
            }
        };

        // keep metrics info to avoid cloning when saving
        cfg_if! {
            if #[cfg(feature = "metrics")] {
                let tx_metrics = tx_execution.metrics();
                let tx_gas = tx_execution.result.execution.gas;
            }
        }

        // persist state
        self.miner.save_execution(tx_execution, false, false)?;

        // track metrics
        #[cfg(feature = "metrics")]
        {
            *block_metrics += tx_metrics;

            metrics::inc_executor_external_transaction(start.elapsed(), tx_contract, tx_function);
            metrics::inc_executor_external_transaction_account_reads(tx_metrics.account_reads, tx_contract, tx_function);
            metrics::inc_executor_external_transaction_slot_reads(tx_metrics.slot_reads, tx_contract, tx_function);
            metrics::inc_executor_external_transaction_gas(tx_gas.as_u64() as usize, tx_contract, tx_function);
        }

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Local transactions
    // -------------------------------------------------------------------------

    /// Executes a transaction persisting state changes.
    #[tracing::instrument(name = "executor::local_transaction", skip_all, fields(tx_hash, tx_from, tx_to, tx_nonce))]
    pub fn execute_local_transaction(&self, tx: TransactionInput) -> Result<(), StratusError> {
        #[cfg(feature = "metrics")]
        let function = codegen::function_sig(&tx.input);
        #[cfg(feature = "metrics")]
        let contract = codegen::contract_name(&tx.to);
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        tracing::debug!(tx_hash = %tx.hash, "executing local transaction");

        // track
        Span::with(|s| {
            s.rec_str("tx_hash", &tx.hash);
            s.rec_str("tx_from", &tx.signer);
            s.rec_opt("tx_to", &tx.to);
            s.rec_str("tx_nonce", &tx.nonce);
        });

        // execute according to the strategy
        const INFINITE_ATTEMPTS: usize = usize::MAX;

        let tx_execution = match self.config.executor_strategy {
            // Executes transactions in serial mode:
            // * Uses a Mutex, so a new transactions starts executing only after the previous one is executed and persisted.
            // * Without a Mutex, conflict can happen because the next transactions starts executing before the previous one is saved.
            // * Conflict detection runs, but it should never trigger because of the Mutex.
            ExecutorStrategy::Serial => {
                // acquire serial execution lock
                let _serial_lock = self.locks.serial.lock();

                // execute transaction
                self.execute_local_transaction_attempts(tx, EvmRoute::Serial, INFINITE_ATTEMPTS)
            }

            // Executes transactions in parallel mode:
            // * Conflict detection prevents data corruption.
            ExecutorStrategy::Paralell => {
                let parallel_attempt = self.execute_local_transaction_attempts(tx.clone(), EvmRoute::Parallel, 1);
                match parallel_attempt {
                    Ok(tx_execution) => Ok(tx_execution),
                    Err(e) =>
                        if let StratusError::Storage(StorageError::TransactionConflict(_)) = e {
                            self.execute_local_transaction_attempts(tx.clone(), EvmRoute::Serial, INFINITE_ATTEMPTS)
                        } else {
                            Err(e)
                        },
                }
            }
        };

        #[cfg(feature = "metrics")]
        metrics::inc_executor_local_transaction(start.elapsed(), tx_execution.is_ok(), contract, function);

        tx_execution
    }

    /// Executes a transaction until it reaches the max number of attempts.
    fn execute_local_transaction_attempts(&self, tx_input: TransactionInput, evm_route: EvmRoute, max_attempts: usize) -> Result<(), StratusError> {
        // validate
        if tx_input.signer.is_zero() {
            return Err(TransactionError::FromZeroAddress.into());
        }

        // executes transaction until no more conflicts
        let mut attempt = 0;
        loop {
            attempt += 1;

            // track
            let _span = debug_span!(
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

            // prepare evm input
            let (pending_header, _) = self.storage.read_pending_block_header();
            let evm_input = EvmInput::from_eth_transaction(&tx_input, &pending_header);

            // execute transaction in evm (retry only in case of conflict, but do not retry on other failures)
            tracing::debug!(
                %attempt,
                tx_hash = %tx_input.hash,
                tx_nonce = %tx_input.nonce,
                tx_from = %tx_input.from,
                tx_signer = %tx_input.signer,
                tx_to = ?tx_input.to,
                tx_data_len = %tx_input.input.len(),
                tx_data = %tx_input.input,
                ?evm_input,
                "executing local transaction attempt"
            );

            let evm_result = self.evms.execute(evm_input.clone(), evm_route)?;

            // save execution to temporary storage
            // in case of failure, retry if conflict or abandon if unexpected error
            let tx_execution = TransactionExecution::new(tx_input.clone(), evm_input, evm_result);
            #[cfg(feature = "metrics")]
            let tx_metrics = tx_execution.metrics();
            #[cfg(feature = "metrics")]
            let gas_used = tx_execution.result.execution.gas;
            #[cfg(feature = "metrics")]
            let function = codegen::function_sig(&tx_input.input);
            #[cfg(feature = "metrics")]
            let contract = codegen::contract_name(&tx_input.to);

            if let ExecutionResult::Reverted { reason } = &tx_execution.result.execution.result {
                tracing::info!(?reason, "Local transaction execution reverted");
                #[cfg(feature = "metrics")]
                metrics::inc_executor_local_transaction_reverts(contract, function, reason.0.as_ref());
            }

            match self.miner.save_execution(tx_execution, matches!(evm_route, EvmRoute::Parallel), true) {
                Ok(_) => {
                    // track metrics
                    #[cfg(feature = "metrics")]
                    {
                        metrics::inc_executor_local_transaction_account_reads(tx_metrics.account_reads, contract, function);
                        metrics::inc_executor_local_transaction_slot_reads(tx_metrics.slot_reads, contract, function);
                        metrics::inc_executor_local_transaction_gas(gas_used.as_u64() as usize, true, contract, function);
                    }
                    return Ok(());
                }
                Err(e) => match e {
                    StratusError::Storage(StorageError::TransactionConflict(ref conflicts)) => {
                        tracing::warn!(%attempt, ?conflicts, "temporary storage conflict detected when saving execution");
                        if attempt >= max_attempts {
                            return Err(e);
                        }
                        continue;
                    }
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

    /// Executes a transaction without persisting state changes.
    #[tracing::instrument(name = "executor::local_call", skip_all, fields(from, to))]
    pub fn execute_local_call(&self, call_input: CallInput, point_in_time: PointInTime) -> Result<EvmExecution, StratusError> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        Span::with(|s| {
            s.rec_opt("from", &call_input.from);
            s.rec_opt("to", &call_input.to);
        });
        tracing::info!(
            from = ?call_input.from,
            to = ?call_input.to,
            data_len = call_input.data.len(),
            data = %call_input.data,
            %point_in_time,
            "executing read-only local transaction"
        );

        // execute
        let evm_input = match point_in_time {
            PointInTime::Pending => {
                let (pending_header, tx_count) = self.storage.read_pending_block_header();
                EvmInput::from_pending_block(call_input.clone(), pending_header, tx_count)
            }
            point_in_time => {
                let Some(block) = self.storage.read_block(point_in_time.into())? else {
                    return Err(RpcError::BlockFilterInvalid { filter: point_in_time.into() }.into());
                };
                EvmInput::from_mined_block(call_input.clone(), block, point_in_time)
            }
        };

        let evm_route = match point_in_time {
            PointInTime::Pending => EvmRoute::CallPresent,
            PointInTime::Mined | PointInTime::MinedPast(_) => EvmRoute::CallPast,
        };
        let evm_result = self.evms.execute(evm_input, evm_route);

        // track metrics
        #[cfg(feature = "metrics")]
        {
            let function = codegen::function_sig(&call_input.data);
            let contract = codegen::contract_name(&call_input.to);

            match &evm_result {
                Ok(evm_result) => {
                    metrics::inc_executor_local_call(start.elapsed(), true, contract, function);
                    metrics::inc_executor_local_call_account_reads(evm_result.metrics.account_reads, contract, function);
                    metrics::inc_executor_local_call_slot_reads(evm_result.metrics.slot_reads, contract, function);
                    metrics::inc_executor_local_call_gas(evm_result.execution.gas.as_u64() as usize, contract, function);
                }
                Err(_) => {
                    metrics::inc_executor_local_call(start.elapsed(), false, contract, function);
                }
            }
        }

        let execution = evm_result?.execution;
        Ok(execution)
    }

    pub fn trace_transaction(&self, tx_hash: Hash, opts: Option<GethDebugTracingOptions>, trace_unsuccessful_only: bool) -> Result<GethTrace, StratusError> {
        Span::with(|s| {
            s.rec_str("tx_hash", &tx_hash);
        });

        tracing::info!("inspecting transaction");
        let opts = opts.unwrap_or_default();
        let tracer_type = opts.tracer.clone();

        timed(|| {
            self.evms.inspect(InspectorInput {
                tx_hash,
                opts,
                trace_unsuccessful_only,
            })
        })
        .with(|m| metrics::inc_evm_inspect(m.elapsed, serde_json::to_string(&tracer_type).unwrap_or_else(|_| "unkown".to_owned())))
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
