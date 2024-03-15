//! EthExecutor: Ethereum Transaction Coordinator
//!
//! This module provides the `EthExecutor` struct, which acts as a coordinator for executing Ethereum transactions.
//! It encapsulates the logic for transaction execution, state mutation, and event notification.
//! `EthExecutor` is designed to work with the `Evm` trait implementations to execute transactions and calls,
//! while also interfacing with a miner component to handle block mining and a storage component to persist state changes.

use std::sync::Arc;
use std::time::Instant;

use anyhow::anyhow;
use tokio::sync::broadcast;
use tokio::sync::oneshot;
use tokio::sync::Mutex;

use crate::eth::evm::EvmExecutionResult;
use crate::eth::evm::EvmInput;
use crate::eth::primitives::Block;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::Execution;
use crate::eth::primitives::ExecutionMetrics;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipts;
use crate::eth::primitives::ExternalTransactionExecution;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionInput;
use crate::eth::storage::StorageError;
use crate::eth::storage::StratusStorage;
use crate::eth::BlockMiner;
use crate::infra::metrics;

/// Number of events in the backlog.
const NOTIFIER_CAPACITY: usize = u16::MAX as usize;

pub type EvmTask = (EvmInput, oneshot::Sender<anyhow::Result<EvmExecutionResult>>);

/// The EthExecutor struct is responsible for orchestrating the execution of Ethereum transactions.
/// It holds references to the EVM, block miner, and storage, managing the overall process of
/// transaction execution, block production, and state management.
pub struct EthExecutor {
    // Channel to send transactions to background EVMs.
    evm_tx: crossbeam_channel::Sender<EvmTask>,

    // Mutex-wrapped miner for creating new blockchain blocks.
    miner: Mutex<BlockMiner>,

    // Shared storage backend for persisting blockchain state.
    storage: Arc<StratusStorage>,

    // Broadcast channels for notifying subscribers about new blocks and logs.
    block_notifier: broadcast::Sender<Block>,
    log_notifier: broadcast::Sender<LogMined>,
}

impl EthExecutor {
    /// Creates a new executor.
    pub fn new(evm_tx: crossbeam_channel::Sender<EvmTask>, storage: Arc<StratusStorage>) -> Self {
        Self {
            evm_tx,
            miner: Mutex::new(BlockMiner::new(Arc::clone(&storage))),
            storage,
            block_notifier: broadcast::channel(NOTIFIER_CAPACITY).0,
            log_notifier: broadcast::channel(NOTIFIER_CAPACITY).0,
        }
    }

    // -------------------------------------------------------------------------
    // Transaction execution
    // -------------------------------------------------------------------------

    /// Re-executes an external block locally and imports it to the storage.
    pub async fn import_external_to_perm(&self, block: ExternalBlock, receipts: &mut ExternalReceipts) -> anyhow::Result<Block> {
        // import block
        let block = self.import_external_to_temp(block, receipts).await?;

        // commit block
        self.storage.set_mined_block_number(*block.number()).await?;
        if let Err(e) = self.storage.commit_to_perm(block.clone()).await {
            let json_block = serde_json::to_string(&block).unwrap();
            tracing::error!(reason = ?e, %json_block);
            return Err(e.into());
        };

        Ok(block)
    }

    /// Re-executes an external block locally.
    pub async fn import_external_to_temp(&self, block: ExternalBlock, receipts: &mut ExternalReceipts) -> anyhow::Result<Block> {
        let start = Instant::now();
        let mut block_metrics = ExecutionMetrics::default();
        tracing::info!(number = %block.number(), "importing external block");

        // re-execute transactions
        let mut executions: Vec<ExternalTransactionExecution> = Vec::with_capacity(block.transactions.len());
        for tx in block.transactions.clone() {
            let tx_start = Instant::now();

            // re-execute transaction
            let receipt = receipts.try_take(&tx.hash())?;
            let evm_input = EvmInput::from_external_transaction(&block, tx.clone(), &receipt)?;
            let execution = self.execute_in_evm(evm_input).await;

            // handle execution result
            match execution {
                Ok((mut execution, execution_metrics)) => {
                    // apply execution costs that were not consided when re-executing the transaction
                    execution.apply_execution_costs(&receipt)?;

                    // ensure it matches receipt before saving
                    if let Err(e) = execution.compare_with_receipt(&receipt) {
                        let json_tx = serde_json::to_string(&tx).unwrap();
                        let json_receipt = serde_json::to_string(&receipt).unwrap();
                        let json_execution_logs = serde_json::to_string(&execution.logs).unwrap();
                        tracing::error!(%json_tx, %json_receipt, %json_execution_logs, "mismatch reexecuting transaction");
                        return Err(e);
                    };

                    // temporarily save state to next transactions from the same block
                    self.storage.save_account_changes_to_temp(execution.changes.clone()).await?;
                    executions.push((tx, receipt, execution));

                    // track metrics
                    metrics::inc_executor_external_transaction(tx_start.elapsed());
                    block_metrics.account_reads += execution_metrics.account_reads;
                    block_metrics.slot_reads += execution_metrics.slot_reads;
                }
                Err(e) => {
                    let json_tx = serde_json::to_string(&tx).unwrap();
                    let json_receipt = serde_json::to_string(&receipt).unwrap();
                    tracing::error!(reason = ?e, %json_tx, %json_receipt, "unexpected error reexecuting transaction");
                    return Err(e);
                }
            }
        }

        // convert block
        let block = Block::from_external(block, executions)?;

        // track metrics
        metrics::inc_executor_external_block(start.elapsed());
        metrics::inc_executor_external_block_account_reads(block_metrics.account_reads);
        metrics::inc_executor_external_block_slot_reads(block_metrics.slot_reads);

        Ok(block)
    }

    /// Executes a transaction persisting state changes.
    pub async fn transact(&self, transaction: TransactionInput) -> anyhow::Result<Execution> {
        let start = Instant::now();
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

        // validates
        if transaction.signer.is_zero() {
            tracing::warn!("rejecting transaction from zero address");
            return Err(anyhow!("transaction sent from zero address is not allowed."));
        }

        // executes transaction until no more conflicts
        // TODO: must have a stop condition like timeout or max number of retries
        let (execution, block) = loop {
            // execute and check conflicts before mining block
            let evm_input = EvmInput::from_eth_transaction(transaction.clone());
            let execution = self.execute_in_evm(evm_input).await?.0;

            // mine and commit block
            let mut miner_lock = self.miner.lock().await;
            let block = miner_lock.mine_with_one_transaction(transaction.clone(), execution.clone()).await?;
            match self.storage.commit_to_perm(block.clone()).await {
                Ok(()) => {}
                Err(StorageError::Conflict(conflicts)) => {
                    tracing::warn!(?conflicts, "storage conflict detected when saving block");
                    continue;
                }
                Err(e) => {
                    metrics::inc_executor_transact(start.elapsed(), false);
                    return Err(e.into());
                }
            };
            break (execution, block);
        };

        // notify new blocks
        if let Err(e) = self.block_notifier.send(block.clone()) {
            tracing::error!(reason = ?e, "failed to send block notification");
        };

        // notify transaction logs
        for trx in block.transactions {
            for log in trx.logs {
                if let Err(e) = self.log_notifier.send(log) {
                    tracing::error!(reason = ?e, "failed to send log notification");
                };
            }
        }

        metrics::inc_executor_transact(start.elapsed(), true);
        Ok(execution)
    }

    /// Executes a transaction without persisting state changes.
    pub async fn call(&self, input: CallInput, point_in_time: StoragePointInTime) -> anyhow::Result<Execution> {
        let start = Instant::now();
        tracing::info!(
            from = ?input.from,
            to = ?input.to,
            data_len = input.data.len(),
            data = %input.data,
            "executing read-only transaction"
        );

        let evm_input = EvmInput::from_eth_call(input, point_in_time);
        let result = self.execute_in_evm(evm_input).await;
        metrics::inc_executor_call(start.elapsed(), result.is_ok());
        result.map(|x| x.0)
    }

    #[cfg(feature = "dev")]
    pub async fn mine_empty_block(&self) -> anyhow::Result<()> {
        let mut miner_lock = self.miner.lock().await;
        let block = miner_lock.mine_with_no_transactions().await?;
        self.storage.commit_to_perm(block.clone()).await?;

        if let Err(e) = self.block_notifier.send(block.clone()) {
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
    async fn execute_in_evm(&self, evm_input: EvmInput) -> anyhow::Result<EvmExecutionResult> {
        let (execution_tx, execution_rx) = oneshot::channel::<anyhow::Result<EvmExecutionResult>>();
        self.evm_tx.send((evm_input, execution_tx))?;
        execution_rx.await?
    }
}
