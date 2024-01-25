//! EthExecutor: Ethereum Transaction Coordinator
//!
//! This module provides the `EthExecutor` struct, which acts as a coordinator for executing Ethereum transactions.
//! It encapsulates the logic for transaction execution, state mutation, and event notification.
//! `EthExecutor` is designed to work with the `Evm` trait implementations to execute transactions and calls,
//! while also interfacing with a miner component to handle block mining and a storage component to persist state changes.

use std::sync::Arc;

use anyhow::anyhow;
use tokio::sync::broadcast;
use tokio::sync::Mutex;

use crate::eth::evm::revm::Revm;
use crate::eth::evm::Evm;
use crate::eth::miner::BlockMiner;
use crate::eth::primitives::Block;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::eth::storage::EthStorage;

/// The EthExecutor struct is responsible for orchestrating the execution of Ethereum transactions.
/// It holds references to the EVM, block miner, and storage, managing the overall process of
/// transaction execution, block production, and state management.
pub struct EthExecutor {
    // Mutex-wrapped EVM for synchronized access to EVM operations.
    evm: Mutex<Box<dyn Evm>>,
    // Mutex-wrapped miner for creating new blockchain blocks.
    miner: Mutex<BlockMiner>,
    // Shared storage backend for persisting blockchain state.
    eth_storage: Arc<dyn EthStorage>,
    // Broadcast channels for notifying subscribers about new blocks and logs.
    block_notifier: broadcast::Sender<Block>,
    log_notifier: broadcast::Sender<LogMined>,
}

const NOTIFIER_CAPACITY: usize = u16::MAX as usize;

impl EthExecutor {
    /// Creates a new executor.
    pub fn new(evm: Box<impl Evm>, eth_storage: Arc<dyn EthStorage>) -> Self {
        Self {
            evm: Mutex::new(evm),
            miner: Mutex::new(BlockMiner::new(Arc::clone(&eth_storage))),
            eth_storage,
            block_notifier: broadcast::channel(NOTIFIER_CAPACITY).0,
            log_notifier: broadcast::channel(NOTIFIER_CAPACITY).0,
        }
    }

    /// Executes Ethereum transactions and facilitates block creation.
    ///
    /// This function is a key part of the transaction processing pipeline. It begins by validating
    /// incoming transactions and then proceeds to execute them. Unlike conventional blockchain systems,
    /// the block creation here is not dictated by timed intervals but is instead triggered by transaction
    /// processing itself. This method encapsulates the execution, block mining, and state mutation,
    /// concluding with broadcasting necessary notifications for the newly created block and associated transaction logs.
    ///
    /// TODO: too much cloning that can be optimized here.
    pub async fn transact(&self, transaction: TransactionInput) -> anyhow::Result<TransactionExecution> {
        tracing::info!(
            hash = %transaction.hash,
            nonce = %transaction.nonce,
            from = %transaction.from,
            signer = %transaction.signer,
            to = ?transaction.to,
            data_len = %transaction.input.len(),
            data = %transaction.input,
            "evm executing transaction"
        );

        // validate
        if transaction.signer.is_zero() {
            tracing::warn!("rejecting transaction from zero address");
            return Err(anyhow!("Transaction sent from zero address is not allowed."));
        }

        // acquire locks
        let mut miner_lock = self.miner.lock().await;
        let mut _evm_lock = self.evm.lock().await;

        let mut evm = Revm::new(Arc::clone(&self.eth_storage));
        let tclone = transaction.clone();

        // execute, mine and save
        let execution = tokio::task::spawn_blocking(move || evm.execute(tclone.try_into()?)).await??;
        let block = miner_lock.mine_with_one_transaction(transaction, execution.clone()).await?;
        self.eth_storage.save_block(block.clone()).await?;

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

        Ok(execution)
    }

    /// Execute a function and return the function output. State changes are ignored.
    pub async fn call(&self, input: CallInput, point_in_time: StoragePointInTime) -> anyhow::Result<TransactionExecution> {
        tracing::info!(
            from = %input.from,
            to = ?input.to,
            data_len = input.data.len(),
            data = %input.data,
            "evm executing call"
        );

        let mut _evm_lock = self.evm.lock().await;
        // execute, but not save
        let mut evm = Revm::new(Arc::clone(&self.eth_storage));

        let execution = tokio::task::spawn_blocking(move || evm.execute((input, point_in_time).into())).await??;
        Ok(execution)
    }

    /// Subscribe to new blocks events.
    pub fn subscribe_to_new_heads(&self) -> broadcast::Receiver<Block> {
        self.block_notifier.subscribe()
    }

    /// Subscribe to new logs events.
    pub fn subscribe_to_logs(&self) -> broadcast::Receiver<LogMined> {
        self.log_notifier.subscribe()
    }
}
