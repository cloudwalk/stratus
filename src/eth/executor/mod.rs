mod config;
mod evm;
mod evm_worker_pool;
pub mod types;

use std::mem;
use std::sync::Arc;

#[cfg(feature = "metrics")]
use alloy_consensus::Transaction;
use alloy_rpc_types_trace::geth::GethDebugTracingOptions;
use alloy_rpc_types_trace::geth::GethTrace;
use anyhow::bail;
pub use config::ExecutorConfig;
use derive_more::Deref;
pub use evm::types::AccessListOutput;
pub use evm::types::CallExecutionOutput;
pub use evm::types::EvmExecutionMetrics;
pub use evm::types::EvmKind;
pub use evm::types::TransactionExecutionInput;
pub use evm::types::TransactionExecutionOutput;
pub use evm::types::TransactionExecutionResult;
use parking_lot::Condvar;
use parking_lot::Mutex;
use tracing::Span;
use tracing::debug_span;
#[cfg(feature = "tracing")]
use tracing::info_span;
pub use types::ExecutionResult;
pub use types::ExecutorError;
pub use types::RevertReason;
pub use types::State;
pub use types::TransactionExecution;

#[cfg(feature = "metrics")]
use crate::eth::codegen;
use crate::eth::executor::evm::RevmResultAndState;
use crate::eth::executor::evm::types::CallExecutionInput;
use crate::eth::executor::evm::types::InspectorInput;
#[cfg(feature = "metrics")]
use crate::eth::executor::evm::types::SlotAccessMetrics;
use crate::eth::executor::evm_worker_pool::EvmWorkerPool;
use crate::eth::executor::types::EvmRoute;
use crate::eth::miner::Miner;
use crate::eth::rpc::RpcError;
use crate::eth::storage::ExecutionKind;
use crate::eth::storage::StorageError;
use crate::eth::storage::StratusStorage;
use crate::eth::types::Address;
use crate::eth::types::BlockNumber;
use crate::eth::types::CallInput;
use crate::eth::types::ExternalBlock;
use crate::eth::types::ExternalReceipt;
use crate::eth::types::ExternalReceipts;
use crate::eth::types::ExternalTransaction;
use crate::eth::types::Hash;
use crate::eth::types::PointInTime;
use crate::eth::types::StratusError;
use crate::eth::types::TransactionInput;
#[cfg(feature = "metrics")]
use crate::ext::OptionExt;
use crate::ext::to_json_string;
use crate::infra::metrics;
use crate::infra::metrics::timed;
use crate::infra::tracing::SpanExt;

// -----------------------------------------------------------------------------
// Executor
// -----------------------------------------------------------------------------

#[derive(Deref, Default)]
struct Semaphore {
    #[deref]
    sem: Arc<SemaphoreInner>,
}

#[derive(Default)]
struct SemaphoreInner {
    permits: Mutex<usize>,
    cvar: Condvar,
}

struct Permit {
    sem: Arc<SemaphoreInner>,
}

impl Semaphore {
    fn new(permits: usize) -> Self {
        Self {
            sem: Arc::new(SemaphoreInner {
                permits: Mutex::new(permits),
                cvar: Condvar::new(),
            }),
        }
    }

    fn acquire(&self) -> Permit {
        let mut permits = self.permits.lock();
        while *permits == 0 {
            self.cvar.wait(&mut permits);
        }
        *permits -= 1;
        drop(permits);
        Permit { sem: self.sem.clone() }
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        let mut permits = self.sem.permits.lock();
        *permits += 1;
        self.sem.cvar.notify_one();
    }
}

/// Locks used for local execution.
#[derive(Default)]
pub struct ExecutorLocks {
    transaction: Mutex<()>,
    transaction_warmup: Semaphore,
}

pub struct Executor {
    /// Executor inner locks.
    locks: ExecutorLocks,

    /// Channels to send transactions to background EVMs.
    evms: EvmWorkerPool,

    /// Mutex-wrapped miner for creating new blockchain blocks.
    miner: Arc<Miner>,

    /// Shared storage backend for persisting blockchain state.
    storage: Arc<StratusStorage>,

    /// Whether to reject transactions and calls targeting accounts that are not contracts.
    reject_not_contract: bool,
}

impl Executor {
    pub fn new(storage: Arc<StratusStorage>, miner: Arc<Miner>, config: ExecutorConfig) -> Self {
        tracing::info!(?config, "creating executor");
        let reject_not_contract = config.executor_reject_not_contract;
        let evms = EvmWorkerPool::spawn(Arc::clone(&storage), &config);
        Self {
            locks: ExecutorLocks {
                transaction_warmup: Semaphore::new(100),
                ..Default::default()
            },
            evms,
            miner,
            storage,
            reject_not_contract,
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
        let (start, mut block_metrics) = (metrics::now(), SlotAccessMetrics::default());

        #[cfg(feature = "tracing")]
        let _span = info_span!("executor::external_block", block_number = %block.number()).entered();
        tracing::info!(block_number = %block.number(), "reexecuting external block");

        self.storage.set_pending_from_external(&block);

        // track pending block
        let block_number = block.number();
        let block_transactions = mem::take(&mut block.transactions);

        // determine how to execute each transaction
        for tx in block_transactions.into_transactions() {
            let receipt = receipts.try_remove(tx.hash())?;
            self.execute_external_transaction(
                tx,
                receipt,
                block_number,
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
        #[cfg(feature = "metrics")] block_metrics: &mut SlotAccessMetrics,
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

        let tx_input: TransactionInput = tx.try_into()?;
        let pending_block = self.storage.read_pending_block_header();
        let mut evm_input = TransactionExecutionInput::from_eth_transaction(&tx_input, pending_block.number, *pending_block.timestamp);

        // when transaction externally failed, create fake transaction instead of reexecuting
        let (tx_execution, state) = match receipt.is_success() {
            // successful external transaction, re-execute locally
            true => {
                // re-execute transaction
                let evm_execution = self.evms.execute::<TransactionExecutionOutput>(EvmRoute::Transaction(evm_input.clone()));

                // handle re-execution result
                let (mut evm_result, evm_metrics) = match evm_execution {
                    Ok((evm_result, evm_metrics)) => (evm_result, evm_metrics),
                    Err(e) => {
                        let json_tx = to_json_string(&tx_input);
                        let json_receipt = to_json_string(&receipt);
                        tracing::error!(reason = ?e, %block_number, tx_hash = %tx_input.transaction_info.hash, %json_tx, %json_receipt, "failed to reexecute external transaction");
                        return Err(e.into());
                    }
                };

                // update execution with receipt
                evm_result.apply_receipt(&receipt)?;

                // ensure it matches receipt before saving
                if let Err(e) = evm_result.compare_with_receipt(&receipt) {
                    let json_tx = to_json_string(&tx_input);
                    let json_receipt = to_json_string(&receipt);
                    let json_execution_logs = to_json_string(&evm_result.logs);
                    tracing::error!(reason = ?e, %block_number, tx_hash = %tx_input.transaction_info.hash, %json_tx, %json_receipt, %json_execution_logs, "failed to reexecute external transaction");
                    return Err(e);
                };

                // track metrics
                #[cfg(feature = "metrics")]
                {
                    *block_metrics += evm_metrics.slot_access;

                    metrics::inc_executor_external_transaction(start.elapsed(), tx_contract, tx_function);
                    metrics::inc_executor_external_transaction_account_reads(evm_metrics.slot_access.account_reads, tx_contract, tx_function);
                    metrics::inc_executor_external_transaction_slot_reads(evm_metrics.slot_access.slot_reads, tx_contract, tx_function);
                    metrics::inc_executor_external_transaction_gas(evm_result.gas_used.as_u64() as usize, tx_contract, tx_function);
                }

                (
                    TransactionExecution::new(tx_input.transaction_info, tx_input.signature, evm_input, evm_result.outcome),
                    evm_result.state,
                )
            }
            //
            // failed external transaction, re-create from receipt without re-executing
            false => {
                let sender = self.storage.read_account(receipt.from.into(), ExecutionKind::Transaction)?;
                if tx_input.execution_info.nonce != sender.nonce {
                    bail!(
                        "reverted external transaction should have the correct nonce. address: {:?}, input: {:?}, sender: {:?}",
                        tx_input.signer(),
                        tx_input.execution_info.nonce,
                        sender.nonce
                    );
                }
                let evm_result = TransactionExecutionOutput::from_failed_external_transaction(sender, &receipt)?;

                evm_input.gas_limit = tx_input.execution_info.gas_limit;
                evm_input.gas_price = tx_input.execution_info.gas_price;

                (
                    TransactionExecution::new(tx_input.transaction_info, tx_input.signature, evm_input, evm_result.outcome),
                    evm_result.state,
                )
            }
        };

        // persist state
        self.miner.save_execution(tx_execution, state)?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Local transactions
    // -------------------------------------------------------------------------

    /// Validates that the target account is a contract, reading it from storage at the given point in time.
    pub fn validate_to_is_contract(&self, to_address: Address, mut kind: ExecutionKind) -> Result<(), StratusError> {
        // small warm up
        if matches!(kind, ExecutionKind::Transaction) {
            kind = ExecutionKind::RPC(PointInTime::Pending);
        }
        let account = self.storage.read_account(to_address, kind)?;
        if account.bytecode.is_none() {
            if self.reject_not_contract {
                return Err(ExecutorError::AccountNotContract { address: to_address }.into());
            } else {
                tracing::warn!(%to_address, "evm to_account is not a contract because does not have bytecode");
            }
        }
        Ok(())
    }

    /// Executes a transaction persisting state changes.
    #[tracing::instrument(name = "executor::local_transaction", skip_all, fields(tx_hash, tx_from, tx_to, tx_nonce))]
    pub fn execute_local_transaction(&self, tx: TransactionInput, access_list: Option<AccessListOutput>) -> Result<(), StratusError> {
        #[cfg(feature = "metrics")]
        let function = codegen::function_sig(&tx.execution_info.input);
        #[cfg(feature = "metrics")]
        let contract = codegen::contract_name(&tx.execution_info.to);

        tracing::debug!(tx_hash = %tx.transaction_info.hash, "executing local transaction");

        // track
        Span::with(|s| {
            s.rec_str("tx_hash", &tx.transaction_info.hash);
            s.rec_str("tx_from", &tx.signer());
            s.rec_opt("tx_to", &tx.execution_info.to);
            s.rec_str("tx_nonce", &tx.execution_info.nonce);
        });

        metrics::inc_executor_local_transaction_semaphore_waiting(1);
        #[cfg(feature = "metrics")]
        let permit = self.locks.transaction_warmup.acquire();
        if let Some(access_list) = access_list {
            self.storage.load_access_list(access_list);
        }

        // execute according to the strategy
        const INFINITE_ATTEMPTS: usize = usize::MAX;

        // Executes transactions serially:
        // * Uses a Mutex, so a new transactions starts executing only after the previous one is executed and persisted.
        // * Without a Mutex, conflict can happen because the next transactions starts executing before the previous one is saved.
        metrics::inc_executor_local_transaction_lock_waiting(1);
        let transaction_lock = self.locks.transaction.lock();
        drop(permit);
        metrics::dec_executor_local_transaction_semaphore_waiting(1);
        #[cfg(feature = "metrics")]
        metrics::dec_executor_local_transaction_lock_waiting(1);

        #[cfg(feature = "metrics")]
        let start = metrics::now();

        // execute transaction
        let tx_execution = self.execute_local_transaction_attempts(tx, INFINITE_ATTEMPTS);

        #[cfg(feature = "metrics")]
        let execution_elapsed = start.elapsed();

        drop(transaction_lock);

        #[cfg(feature = "metrics")]
        metrics::inc_executor_local_transaction(execution_elapsed, tx_execution.is_ok(), contract, function);

        tx_execution
    }

    /// Executes a transaction until it reaches the max number of attempts.
    fn execute_local_transaction_attempts(&self, tx_input: TransactionInput, max_attempts: usize) -> Result<(), StratusError> {
        // validate
        if tx_input.signer().is_zero() {
            return Err(ExecutorError::FromZeroAddress.into());
        }

        // executes transaction until no more conflicts
        let mut attempt = 0;
        loop {
            attempt += 1;

            // track
            let _span = debug_span!(
                "executor::local_transaction_attempt",
                %attempt,
                tx_hash = %tx_input.transaction_info.hash,
                tx_from = %tx_input.signer(),
                tx_to = tracing::field::Empty,
                tx_nonce = %tx_input.execution_info.nonce
            )
            .entered();
            Span::with(|s| {
                s.rec_opt("tx_to", &tx_input.execution_info.to);
            });

            // prepare evm input
            let pending_header = self.storage.read_pending_block_header();
            let evm_input = TransactionExecutionInput::from_eth_transaction(&tx_input, pending_header.number, *pending_header.timestamp);

            // execute transaction in evm (retry only in case of conflict, but do not retry on other failures)
            tracing::debug!(
                %attempt,
                tx_hash = %tx_input.transaction_info.hash,
                tx_nonce = %tx_input.execution_info.nonce,
                tx_signer = %tx_input.signer(),
                tx_to = ?tx_input.execution_info.to,
                tx_data_len = %tx_input.execution_info.input.len(),
                tx_data = %tx_input.execution_info.input,
                ?evm_input,
                "executing local transaction attempt"
            );

            let (evm_result, evm_metrics): (TransactionExecutionOutput, EvmExecutionMetrics) = self.evms.execute(EvmRoute::Transaction(evm_input.clone()))?;

            // save execution to temporary storage
            // in case of failure, retry if conflict or abandon if unexpected error
            let tx_execution = TransactionExecution::new(tx_input.transaction_info, tx_input.signature, evm_input, evm_result.outcome);

            #[cfg(feature = "metrics")]
            let gas_used = tx_execution.output.gas_used;
            #[cfg(feature = "metrics")]
            let function = codegen::function_sig(&tx_input.execution_info.input);
            #[cfg(feature = "metrics")]
            let contract = codegen::contract_name(&tx_input.execution_info.to);

            if let ExecutionResult::Reverted { reason } = &tx_execution.output.result {
                tracing::info!(?reason, "local transaction execution reverted");
                #[cfg(feature = "metrics")]
                metrics::inc_executor_local_transaction_reverts(contract, function, reason.0.as_ref());
            }

            match self.miner.save_execution(tx_execution, evm_result.state) {
                Ok(_) => {
                    // track metrics
                    #[cfg(feature = "metrics")]
                    {
                        metrics::inc_executor_local_transaction_account_reads(evm_metrics.slot_access.account_reads, contract, function);
                        metrics::inc_executor_local_transaction_slot_reads(evm_metrics.slot_access.slot_reads, contract, function);
                        metrics::inc_executor_local_transaction_gas(gas_used.as_u64() as usize, true, contract, function);
                    }
                    return Ok(());
                }
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

    /// Executes a read-only call in the local EVM, without persisting state changes.
    ///
    /// When `skip_transient_lock` is set, storage reads do not acquire the transient state lock.
    /// Only meant for access-list computation, where only the set of touched accounts/slots
    /// matters and not their values, so the call does not have to wait for a block being saved.
    #[tracing::instrument(name = "executor::local_call", skip_all, fields(from, to))]
    pub fn execute_local_call<Output>(&self, call_input: CallInput, point_in_time: PointInTime, skip_transient_lock: bool) -> Result<Output, StratusError>
    where
        Output: TryFrom<RevmResultAndState, Error = StratusError>,
    {
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

        #[cfg(feature = "metrics")]
        let (function, contract) = { (codegen::function_sig(&call_input.data), codegen::contract_name(&call_input.to)) };

        // execute
        let mut evm_input = match point_in_time {
            PointInTime::Pending => {
                let pending_header = self.storage.read_pending_block_header();
                CallExecutionInput::from_pending_block(call_input, pending_header)
            }
            _ => {
                let Some(block) = self.storage.read_block(point_in_time.into())? else {
                    return Err(RpcError::BlockFilterInvalid { filter: point_in_time.into() }.into());
                };
                CallExecutionInput::from_mined_block(call_input, block.header, point_in_time)
            }
        };

        // access-list calls only need the set of touched keys, not their values: use the lock-free
        // RPC read kind so they do not wait on the transient state lock held while saving a block
        if skip_transient_lock {
            evm_input.kind = ExecutionKind::RPC(PointInTime::Latest);
        }

        let evm_route = match point_in_time {
            PointInTime::Pending | PointInTime::Latest => EvmRoute::CallPresent(evm_input),
            PointInTime::Past(_) => EvmRoute::CallPast(evm_input),
        };
        let evm_result = self.evms.execute::<Output>(evm_route);

        // track metrics
        #[cfg(feature = "metrics")]
        {
            match &evm_result {
                Ok((_, evm_metrics)) => {
                    metrics::inc_executor_local_call(start.elapsed(), true, contract, function);
                    metrics::inc_executor_local_call_account_reads(evm_metrics.slot_access.account_reads, contract, function);
                    metrics::inc_executor_local_call_slot_reads(evm_metrics.slot_access.slot_reads, contract, function);
                    metrics::inc_executor_local_call_gas(evm_metrics.gas_used.as_u64() as usize, contract, function);
                }
                Err(_) => {
                    metrics::inc_executor_local_call(start.elapsed(), false, contract, function);
                }
            }
        }

        Ok(evm_result?.0)
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
        .with(|m| metrics::inc_executor_inspect(m.elapsed, serde_json::to_string(&tracer_type).unwrap_or_else(|_| "unkown".to_owned())))
    }
}
