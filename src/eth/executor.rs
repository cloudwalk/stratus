#![allow(dead_code)]
#![allow(unused_imports)]
// TODO: Remove clippy `allow` after feature-flags are enabled.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::anyhow;
use futures::StreamExt;
use itertools::Itertools;
use revm::primitives::bitvec::vec;
use tokio::sync::broadcast;
use tokio::sync::oneshot;
use tokio::sync::Mutex;

use crate::eth::evm;
use crate::eth::evm::EvmExecutionResult;
use crate::eth::evm::EvmInput;
use crate::eth::primitives::Block;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::ExecutionConflicts;
use crate::eth::primitives::ExecutionMetrics;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalReceipts;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::ExternalTransactionExecution;
use crate::eth::primitives::LocalTransactionExecution;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::eth::storage::StorageError;
use crate::eth::storage::StratusStorage;
use crate::eth::BlockMiner;
use crate::eth::TransactionRelayer;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::BlockchainClient;

pub type EvmTask = (EvmInput, oneshot::Sender<anyhow::Result<EvmExecutionResult>>);
pub struct Executor {
    /// Channel to send transactions to background EVMs.
    evm_tx: crossbeam_channel::Sender<EvmTask>,

    // Number of running EVMs.
    num_evms: usize,

    /// Mutex-wrapped miner for creating new blockchain blocks.
    miner: Mutex<Arc<BlockMiner>>,

    /// Provider for sending rpc calls to substrate
    relayer: Option<Arc<TransactionRelayer>>,

    /// Shared storage backend for persisting blockchain state.
    storage: Arc<StratusStorage>,
}

impl Executor {
    /// Creates a new [`Executor`].
    pub fn new(
        storage: Arc<StratusStorage>,
        miner: Arc<BlockMiner>,
        relayer: Option<Arc<TransactionRelayer>>,
        evm_tx: crossbeam_channel::Sender<EvmTask>,
        num_evms: usize,
    ) -> Self {
        tracing::info!(%num_evms, "starting executor");
        Self {
            evm_tx,
            num_evms,
            miner: Mutex::new(miner),
            storage,
            relayer,
        }
    }

    // -------------------------------------------------------------------------
    // External transactions
    // -------------------------------------------------------------------------

    /// Reexecutes an external block locally and imports it to the temporary storage.
    #[tracing::instrument(skip_all)]
    pub async fn reexecute_external(&self, block: &ExternalBlock, receipts: &ExternalReceipts) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        let (start, mut block_metrics) = (metrics::now(), ExecutionMetrics::default());

        tracing::info!(number = %block.number(), "reexecuting external block");

        // track active block number
        let storage = &self.storage;
        storage.set_active_external_block(block.clone()).await?;
        storage.set_active_block_number(block.number()).await?;

        // determine how to execute each transaction
        let tx_routes = route_transactions(&block.transactions, receipts)?;

        // execute parallel executions
        let mut tx_parallel_executions = Vec::with_capacity(block.transactions.len());
        for tx_route in &tx_routes {
            if let ParallelExecutionRoute::Parallel(tx, receipt) = tx_route {
                tx_parallel_executions.push(self.reexecute_external_tx(tx, receipt, block));
            }
        }
        let mut parallel_executions = futures::stream::iter(tx_parallel_executions).buffered(self.num_evms);

        // execute serial transactions joining them with parallel
        for tx_route in tx_routes {
            let tx = match tx_route {
                // serial: execute now
                ParallelExecutionRoute::Serial(tx, receipt) => self.reexecute_external_tx(tx, receipt, block).await.map_err(|(_, _, e)| e)?,

                // parallel: get parallel execution result and reexecute if failed because of error
                ParallelExecutionRoute::Parallel(..) => {
                    match parallel_executions.next().await.unwrap() {
                        // success: check conflicts
                        Ok(tx) => match storage.check_conflicts(&tx.result.execution).await? {
                            // no conflict: proceeed
                            None => tx,
                            // conflict: reexecute
                            Some(conflicts) => {
                                tracing::warn!(?conflicts, "re-executing serially because parallel execution conflicted");
                                self.reexecute_external_tx(&tx.tx, &tx.receipt, block).await.map_err(|(_, _, e)| e)?
                            }
                        },

                        // failure: reexecute
                        Err((tx, receipt, e)) => {
                            tracing::warn!(reason = ?e, "re-executing serially because parallel execution errored");
                            self.reexecute_external_tx(tx, receipt, block).await.map_err(|(_, _, e)| e)?
                        }
                    }
                }
            };

            // track transaction metrics
            #[cfg(feature = "metrics")]
            {
                block_metrics += tx.result.metrics;
            }

            // persist state
            storage.save_execution(TransactionExecution::External(tx)).await?;
        }

        // track block metrics
        #[cfg(feature = "metrics")]
        {
            metrics::inc_executor_external_block(start.elapsed());
            metrics::inc_executor_external_block_account_reads(block_metrics.account_reads);
            metrics::inc_executor_external_block_slot_reads(block_metrics.slot_reads);
            metrics::inc_executor_external_block_slot_reads_cached(block_metrics.slot_reads_cached);
        }

        Ok(())
    }

    /// Reexecutes an external transaction locally ensuring it produces the same output.
    async fn reexecute_external_tx<'a, 'b>(
        &'a self,
        tx: &'b ExternalTransaction,
        receipt: &'b ExternalReceipt,
        block: &ExternalBlock,
    ) -> Result<ExternalTransactionExecution, (&'b ExternalTransaction, &'b ExternalReceipt, anyhow::Error)> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        // when transaction externally failed, create fake transaction instead of reexecuting
        if receipt.is_failure() {
            let sender = match self.storage.read_account(&receipt.from.into(), &StoragePointInTime::Present).await {
                Ok(sender) => sender,
                Err(e) => return Err((tx, receipt, e)),
            };
            let execution = match EvmExecution::from_failed_external_transaction(sender, receipt, block) {
                Ok(execution) => execution,
                Err(e) => return Err((tx, receipt, e)),
            };
            let evm_result = EvmExecutionResult {
                execution,
                metrics: ExecutionMetrics::default(),
            };
            return Ok(ExternalTransactionExecution::new(tx.clone(), receipt.clone(), evm_result));
        }

        // reexecute transaction
        let evm_input = match EvmInput::from_external(tx, receipt, block) {
            Ok(evm_input) => evm_input,
            Err(e) => return Err((tx, receipt, e)),
        };

        #[cfg(feature = "metrics")]
        let function = evm_input.extract_function();

        let evm_result = self.execute_in_evm(evm_input).await;

        // handle reexecution result
        match evm_result {
            Ok(mut evm_result) => {
                // track metrics before execution is update with receipt
                #[cfg(feature = "metrics")]
                {
                    metrics::inc_executor_external_transaction_gas(evm_result.execution.gas.as_u64() as usize, function.clone());
                    metrics::inc_executor_external_transaction(start.elapsed(), function);
                }

                // update execution with receipt info
                if let Err(e) = evm_result.execution.apply_receipt(receipt) {
                    return Err((tx, receipt, e));
                };

                // ensure it matches receipt before saving
                if let Err(e) = evm_result.execution.compare_with_receipt(receipt) {
                    let json_tx = serde_json::to_string(&tx).unwrap();
                    let json_receipt = serde_json::to_string(&receipt).unwrap();
                    let json_execution_logs = serde_json::to_string(&evm_result.execution.logs).unwrap();
                    tracing::error!(%json_tx, %json_receipt, %json_execution_logs, "mismatch reexecuting transaction");
                    return Err((tx, receipt, e));
                };

                Ok(ExternalTransactionExecution::new(tx.clone(), receipt.clone(), evm_result))
            }
            Err(e) => {
                let json_tx = serde_json::to_string(&tx).unwrap();
                let json_receipt = serde_json::to_string(&receipt).unwrap();
                tracing::error!(reason = ?e, %json_tx, %json_receipt, "unexpected error reexecuting transaction");
                Err((tx, receipt, e))
            }
        }
    }

    // -------------------------------------------------------------------------
    // Direct transactions
    // -------------------------------------------------------------------------

    /// Executes a transaction persisting state changes.
    #[tracing::instrument(skip_all)]
    pub async fn transact(&self, tx_input: TransactionInput) -> anyhow::Result<TransactionExecution> {
        #[cfg(feature = "metrics")]
        let (start, function) = (metrics::now(), tx_input.extract_function());

        tracing::info!(
            hash = %tx_input.hash,
            nonce = %tx_input.nonce,
            from = ?tx_input.from,
            signer = %tx_input.signer,
            to = ?tx_input.to,
            data_len = %tx_input.input.len(),
            data = %tx_input.input,
            "executing transaction"
        );

        // validate
        if tx_input.signer.is_zero() {
            tracing::warn!("rejecting transaction from zero address");
            return Err(anyhow!("transaction sent from zero address is not allowed."));
        }

        let tx_execution = match &self.relayer {
            // relayer not present
            None => {
                // executes transaction until no more conflicts
                // TODO: must have a stop condition like timeout or max number of retries
                loop {
                    // execute transaction
                    let evm_input = EvmInput::from_eth_transaction(tx_input.clone());
                    let evm_result = self.execute_in_evm(evm_input).await?;

                    // save execution to temporary storage (not working yet)
                    let tx_execution = TransactionExecution::new_local(tx_input.clone(), evm_result.clone());
                    if let Err(e) = self.storage.save_execution(tx_execution.clone()).await {
                        if let Some(StorageError::Conflict(conflicts)) = e.downcast_ref::<StorageError>() {
                            tracing::warn!(?conflicts, "temporary storage conflict detected when saving execution");
                            continue;
                        } else {
                            #[cfg(feature = "metrics")]
                            metrics::inc_executor_transact(start.elapsed(), false, function);
                            return Err(e);
                        }
                    }

                    // TODO: remove automine
                    let miner = self.miner.lock().await;
                    miner.mine_local_and_commit().await?;

                    break tx_execution;
                }
            }
            // relayer present
            Some(relayer) => {
                let evm_input = EvmInput::from_eth_transaction(tx_input.clone());
                let evm_result = self.execute_in_evm(evm_input).await?;
                relayer.forward(tx_input.clone(), evm_result.clone()).await?; // TODO: Check if we should run this in paralel by spawning a task when running the online importer.
                TransactionExecution::new_local(tx_input, evm_result)
            }
        };

        #[cfg(feature = "metrics")]
        metrics::inc_executor_transact(start.elapsed(), true, function.clone());
        #[cfg(feature = "metrics")]
        metrics::inc_executor_transact_gas(tx_execution.execution().gas.as_u64() as usize, true, function);

        Ok(tx_execution)
    }

    /// Executes a transaction without persisting state changes.
    #[tracing::instrument(skip_all)]
    pub async fn call(&self, input: CallInput, point_in_time: StoragePointInTime) -> anyhow::Result<EvmExecution> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();
        #[cfg(feature = "metrics")]
        let function = input.extract_function();

        tracing::info!(
            from = ?input.from,
            to = ?input.to,
            data_len = input.data.len(),
            data = %input.data,
            ?point_in_time,
            "executing read-only transaction"
        );

        let evm_input = EvmInput::from_eth_call(input, point_in_time);
        let evm_result = self.execute_in_evm(evm_input).await;

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

    /// Submits a transaction to the EVM and awaits for its execution.
    #[tracing::instrument(skip_all)]
    async fn execute_in_evm(&self, evm_input: EvmInput) -> anyhow::Result<EvmExecutionResult> {
        let (execution_tx, execution_rx) = oneshot::channel::<anyhow::Result<EvmExecutionResult>>();
        self.evm_tx.send((evm_input, execution_tx))?;
        execution_rx.await?
    }
}

#[cfg(not(feature = "executor-parallel"))]
fn route_transactions<'a>(transactions: &'a [ExternalTransaction], receipts: &'a ExternalReceipts) -> anyhow::Result<Vec<ParallelExecutionRoute<'a>>> {
    let mut routes = Vec::with_capacity(transactions.len());
    for tx in transactions {
        let receipt = receipts.try_get(&tx.hash())?;
        routes.push(ParallelExecutionRoute::Serial(tx, receipt));
    }
    Ok(routes)
}

#[cfg(feature = "executor-parallel")]
fn route_transactions<'a>(transactions: &'a [ExternalTransaction], receipts: &'a ExternalReceipts) -> anyhow::Result<Vec<ParallelExecutionRoute<'a>>> {
    // no transactions
    if transactions.is_empty() {
        return Ok(vec![]);
    }

    // single transaction
    if transactions.len() == 1 {
        let tx = &transactions[0];
        let receipt = receipts.try_get(&tx.hash())?;
        return Ok(vec![ParallelExecutionRoute::Serial(tx, receipt)]);
    }

    // multiple transactions
    let mut routes = Vec::with_capacity(transactions.len());
    let mut seen_from = HashSet::with_capacity(transactions.len());
    for tx in transactions {
        let receipt = receipts.try_get(&tx.hash())?;

        let mut route = ParallelExecutionRoute::Parallel(tx, receipt);
        // transactions from same sender will conflict, so execute serially
        if seen_from.contains(&tx.from) {
            route = ParallelExecutionRoute::Serial(tx, receipt);
        }
        // failed receipts are not reexecuted, so they can be executed in parallel
        if receipt.is_failure() {
            route = ParallelExecutionRoute::Parallel(tx, receipt);
        }

        // track seen data
        seen_from.insert(tx.from);

        routes.push(route);
    }

    Ok(routes)
}

/// How a transaction should be executed in a parallel execution context.
#[derive(Debug, strum::Display, strum::EnumIs)]
enum ParallelExecutionRoute<'a> {
    /// Transaction must be executed serially after all previous states are computed.
    #[strum(to_string = "Serial")]
    Serial(&'a ExternalTransaction, &'a ExternalReceipt),

    /// Transaction can be executed in parallel with other transactions.
    #[strum(to_string = "Parallel")]
    Parallel(&'a ExternalTransaction, &'a ExternalReceipt),
}
