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
use crate::eth::primitives::ExecutionMetrics;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalReceipts;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::TransactionKind;
use crate::eth::storage::StorageError;
use crate::eth::storage::StratusStorage;
use crate::eth::BlockMiner;
use crate::eth::TransactionRelayer;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::BlockchainClient;

/// Number of events in the backlog.
const NOTIFIER_CAPACITY: usize = u16::MAX as usize;

pub type EvmTask = (EvmInput, oneshot::Sender<anyhow::Result<EvmExecutionResult>>);
pub struct Executor {
    /// Channel to send transactions to background EVMs.
    evm_tx: crossbeam_channel::Sender<EvmTask>,

    // Number of running EVMs.
    num_evms: usize,

    /// Mutex-wrapped miner for creating new blockchain blocks.
    miner: Arc<BlockMiner>,

    /// Provider for sending rpc calls to substrate
    relayer: Option<Arc<TransactionRelayer>>,

    /// Shared storage backend for persisting blockchain state.
    storage: Arc<StratusStorage>,

    /// Broadcast channels for notifying subscribers about new blocks and logs.
    block_notifier: broadcast::Sender<Block>,
    log_notifier: broadcast::Sender<LogMined>,
}

impl Executor {
    /// Creates a new [`Executor`].
    pub fn new(
        storage: Arc<StratusStorage>,
        miner: Arc<BlockMiner>,
        relayer: Option<Arc<TransactionRelayer>>,
        evm_tx: crossbeam_channel::Sender<EvmTask>,
        num_evms: usize
    ) -> Self {
        tracing::info!(%num_evms, "creating executor");

        Self {
            evm_tx,
            num_evms,
            miner,
            storage,
            block_notifier: broadcast::channel(NOTIFIER_CAPACITY).0,
            log_notifier: broadcast::channel(NOTIFIER_CAPACITY).0,
            relayer,
        }
    }

    // -------------------------------------------------------------------------
    // External transactions
    // -------------------------------------------------------------------------

    /// Re-executes an external block locally and imports it to the permanent storage.
    ///
    /// TODO: this method will be removed.
    #[tracing::instrument(skip_all)]
    pub async fn import_external_to_perm(&self, block: &ExternalBlock, receipts: &ExternalReceipts) -> anyhow::Result<Block> {
        // import block
        self.reexecute_external_transactions(block, receipts).await?;
        let mut block = self.miner.mine_external(block).await?;

        // import relay failed transactions
        if let Some(relay) = &self.relayer {
            for (tx, ex) in relay.drain_failed_transactions().await {
                block.push_execution(tx, ex);
            }
        }

        // commit block
        self.miner.commit(block.clone()).await?;
        Ok(block)
    }

    /// Re-executes an external block locally and imports it to the temporary storage.
    #[tracing::instrument(skip_all)]
    pub async fn reexecute_external_transactions(&self, block: &ExternalBlock, receipts: &ExternalReceipts) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        let (start, mut block_metrics) = (metrics::now(), ExecutionMetrics::default());

        tracing::info!(number = %block.number(), "importing external block");

        let storage = &self.storage;

        // track active block number
        storage.set_active_block_number(block.number()).await?;

        // execute mixing serial and parallel approaches
        let tx_routes = route_transactions(&block.transactions, receipts)?;
        let mut tx_parallel_executions = Vec::with_capacity(block.transactions.len());

        // execute parallel executions
        for tx_route in &tx_routes {
            if let ParallelExecutionRoute::Parallel(tx, receipt) = tx_route {
                tx_parallel_executions.push(self.reexecute_external(tx, receipt, block));
            }
        }
        let mut parallel_executions = futures::stream::iter(tx_parallel_executions).buffered(self.num_evms);

        // execute serial transactions joining them with parallel
        for tx_route in tx_routes {
            match tx_route {
                // serial: execute now
                ParallelExecutionRoute::Serial(tx, receipt) => {
                    let evm_result = self.reexecute_external(tx, receipt, block).await.2?;

                    // persist state
                    let tx_execution = TransactionExecution::new_external(tx.clone(), receipt.clone(), evm_result.execution);
                    storage.save_execution_to_temp(tx_execution).await?;
                }

                // parallel: check results and re-execute if necessary
                ParallelExecutionRoute::Parallel(..) => {
                    let evm_result = parallel_executions.next().await.unwrap();

                    let decision = match evm_result {
                        // execution success
                        (tx, receipt, Ok(evm_result)) => {
                            let current_execution = &evm_result.execution;

                            // check conflict with all previous transactions
                            // TODO: conflict detection in the temporary storage will avoid checking conflict with all previous transactions here
                            let mut reexecute = false;

                            let prev_txs = storage.temp.read_executions().await;
                            for prev_tx in prev_txs {
                                let prev_execution = &prev_tx.execution;
                                if let Some(conflicts) = prev_execution.check_conflicts(current_execution) {
                                    tracing::warn!(?conflicts, "parallel execution conflicts");
                                    reexecute = true;
                                    break;
                                }
                            }
                            if reexecute {
                                ParallelExecutionDecision::Reexecute(tx, receipt)
                            } else {
                                ParallelExecutionDecision::Proceed(tx, receipt, evm_result)
                            }
                        }
                        // execution failure
                        (tx, receipt, Err(e)) => {
                            tracing::warn!(reason = ?e, "parallel execution failed");
                            ParallelExecutionDecision::Reexecute(tx, receipt)
                        }
                    };

                    // re-execute if necessary
                    let (tx, receipt, evm_result) = match decision {
                        ParallelExecutionDecision::Proceed(tx, receipt, evm_result) => (tx, receipt, evm_result),
                        ParallelExecutionDecision::Reexecute(tx, receipt) => match self.reexecute_external(tx, receipt, block).await {
                            (tx, receipt, Ok(evm_result)) => (tx, receipt, evm_result),
                            (.., Err(e)) => return Err(e),
                        },
                    };

                    // track metrics
                    #[cfg(feature = "metrics")]
                    {
                        block_metrics += evm_result.metrics;
                    }

                    // persist state
                    let tx_execution = TransactionExecution::new_external(tx.clone(), receipt.clone(), evm_result.execution);
                    storage.save_execution_to_temp(tx_execution).await?;
                }
            }
        }

        // track metrics
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
    pub async fn reexecute_external<'a, 'b>(
        &'a self,
        tx: &'b ExternalTransaction,
        receipt: &'b ExternalReceipt,
        block: &ExternalBlock,
    ) -> (&'b ExternalTransaction, &'b ExternalReceipt, anyhow::Result<EvmExecutionResult>) {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        // re-execute transaction or create a fake execution from the failed external transaction
        let evm_result = if receipt.is_success() {
            let evm_input = match EvmInput::from_external_transaction(tx, receipt, block) {
                Ok(evm_input) => evm_input,
                Err(e) => return (tx, receipt, Err(e)),
            };
            self.execute_in_evm(evm_input).await
        } else {
            let sender = match self.storage.read_account(&receipt.from.into(), &StoragePointInTime::Present).await {
                Ok(sender) => sender,
                Err(e) => return (tx, receipt, Err(e)),
            };
            let execution = match EvmExecution::from_failed_external_transaction(sender, receipt, block) {
                Ok(execution) => execution,
                Err(e) => return (tx, receipt, Err(e)),
            };
            Ok(EvmExecutionResult {
                execution,
                metrics: ExecutionMetrics::default(),
            })
        };

        // handle execution result
        match evm_result {
            Ok(mut evm_result) => {
                // apply execution costs that were not consided when re-executing the transaction
                if let Err(e) = evm_result.execution.apply_execution_costs(receipt) {
                    return (tx, receipt, Err(e));
                };
                evm_result.execution.gas = match receipt.gas_used.unwrap_or_default().try_into() {
                    Ok(gas) => gas,
                    Err(e) => return (tx, receipt, Err(e)),
                };

                // ensure it matches receipt before saving
                if let Err(e) = evm_result.execution.compare_with_receipt(receipt) {
                    let json_tx = serde_json::to_string(&tx).unwrap();
                    let json_receipt = serde_json::to_string(&receipt).unwrap();
                    let json_execution_logs = serde_json::to_string(&evm_result.execution.logs).unwrap();
                    tracing::error!(%json_tx, %json_receipt, %json_execution_logs, "mismatch reexecuting transaction");
                    return (tx, receipt, Err(e));
                };

                // track metrics
                #[cfg(feature = "metrics")]
                metrics::inc_executor_external_transaction(start.elapsed());

                (tx, receipt, Ok(evm_result))
            }
            Err(e) => {
                let json_tx = serde_json::to_string(&tx).unwrap();
                let json_receipt = serde_json::to_string(&receipt).unwrap();
                tracing::error!(reason = ?e, %json_tx, %json_receipt, "unexpected error reexecuting transaction");
                (tx, receipt, Err(e))
            }
        }
    }

    // -------------------------------------------------------------------------
    // Direct transactions
    // -------------------------------------------------------------------------

    /// Executes a transaction persisting state changes.
    #[tracing::instrument(skip_all)]
    pub async fn transact(&self, transaction: TransactionInput) -> anyhow::Result<EvmExecution> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        tracing::info!(
            hash = %transaction.hash,
            nonce = %transaction.nonce,
            from = ?transaction.from,
            signer = %transaction.signer,
            to = ?transaction.to,
            data_len = %transaction.input.len(),
            data = %transaction.input,
            "executing transaction"
        );

        // validate
        if transaction.signer.is_zero() {
            tracing::warn!("rejecting transaction from zero address");
            return Err(anyhow!("transaction sent from zero address is not allowed."));
        }

        let execution = if let Some(relay) = &self.relayer {
            let evm_input = EvmInput::from_eth_transaction(transaction.clone());
            let execution = self.execute_in_evm(evm_input).await?.execution;
            relay.forward_transaction(execution.clone(), transaction).await?; // TODO: Check if we should run this in paralel by spawning a task when running the online importer.
            execution
        } else {
            // executes transaction until no more conflicts
            // TODO: must have a stop condition like timeout or max number of retries
            let (execution, block) = loop {
                // execute and check conflicts before mining block
                let evm_input = EvmInput::from_eth_transaction(transaction.clone());
                let execution = self.execute_in_evm(evm_input).await?.execution;

                // mine and commit block
                let block = self.miner.mine_with_one_transaction(transaction.clone(), execution.clone()).await?;
                match self.storage.commit_to_perm(block.clone()).await {
                    Ok(()) => {}
                    Err(StorageError::Conflict(conflicts)) => {
                        tracing::warn!(?conflicts, "storage conflict detected when saving block");
                        continue;
                    }
                    Err(e) => {
                        #[cfg(feature = "metrics")]
                        metrics::inc_executor_transact(start.elapsed(), false);
                        return Err(e.into());
                    }
                };
                break (execution, block);
            };

            // notify new blocks
            // TODO: remove notifications from here because miner will send notifications
            let _ = self.block_notifier.send(block.clone());

            // notify transaction logs
            // TODO: remove notifications from here because miner will send notifications
            for trx in block.transactions {
                for log in trx.logs {
                    let _ = self.log_notifier.send(log);
                }
            }
            execution
        };

        #[cfg(feature = "metrics")]
        metrics::inc_executor_transact(start.elapsed(), true);

        Ok(execution)
    }

    /// Executes a transaction without persisting state changes.
    #[tracing::instrument(skip_all)]
    pub async fn call(&self, input: CallInput, point_in_time: StoragePointInTime) -> anyhow::Result<EvmExecution> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        tracing::info!(
            from = ?input.from,
            to = ?input.to,
            data_len = input.data.len(),
            data = %input.data,
            "executing read-only transaction"
        );

        let evm_input = EvmInput::from_eth_call(input, point_in_time);
        let evm_result = self.execute_in_evm(evm_input).await;

        #[cfg(feature = "metrics")]
        metrics::inc_executor_call(start.elapsed(), evm_result.is_ok());

        evm_result.map(|x| x.execution)
    }

    #[cfg(feature = "dev")]
    pub async fn mine_empty_block(&self) -> anyhow::Result<()> {
        let block = self.miner.mine_empty().await?;
        self.miner.commit(block.clone()).await?;

        // TODO: remove notifications from here because miner will send notifications
        if let Err(e) = self.block_notifier.send(block) {
            tracing::error!(reason = ?e, "failed to send block notification");
        };

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Subscriptions
    // -------------------------------------------------------------------------

    /// Subscribe to new blocks events.
    pub fn subscribe_to_new_heads(&self) -> broadcast::Receiver<Block> {
        self.block_notifier.subscribe()
    }

    /// Subscribe to new logs events.
    pub fn subscribe_to_logs(&self) -> broadcast::Receiver<LogMined> {
        self.log_notifier.subscribe()
    }

    // -------------------------------------------------------------------------
    // Private
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
        // failed receipts are not re-executed, so they can be executed in parallel
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
#[derive(Debug, strum::Display)]
enum ParallelExecutionRoute<'a> {
    /// Transaction must be executed serially after all previous states are computed.
    #[strum(to_string = "Serial")]
    Serial(&'a ExternalTransaction, &'a ExternalReceipt),

    /// Transaction can be executed in parallel with other transactions.
    #[strum(to_string = "Parallel")]
    Parallel(&'a ExternalTransaction, &'a ExternalReceipt),
}

/// What to do after a transaction was executed in parallel.
#[derive(Debug)]
enum ParallelExecutionDecision<'a> {
    /// Parallel execution succeeded and can be persisted.
    Proceed(&'a ExternalTransaction, &'a ExternalReceipt, EvmExecutionResult),

    /// Parallel execution failed and must be re-executed in a serial manner.
    Reexecute(&'a ExternalTransaction, &'a ExternalReceipt),
}
