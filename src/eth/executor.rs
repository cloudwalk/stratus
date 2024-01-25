//! EthExecutor: Ethereum Transaction Coordinator
//!
//! This module provides the `EthExecutor` struct, which acts as a coordinator for executing Ethereum transactions.
//! It encapsulates the logic for transaction execution, state mutation, and event notification.
//! `EthExecutor` is designed to work with the `Evm` trait implementations to execute transactions and calls,
//! while also interfacing with a miner component to handle block mining and a storage component to persist state changes.

use std::sync::Arc;

use anyhow::anyhow;
use nonempty::NonEmpty;
use tokio::runtime::Runtime;
use tokio::sync::broadcast;
use tokio::sync::oneshot;
use tokio::sync::Mutex;

use crate::eth::evm::Evm;
use crate::eth::evm::EvmInput;
use crate::eth::miner::BlockMiner;
use crate::eth::primitives::Block;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::eth::storage::EthStorage;
use crate::ext::new_tokio_runtime;

/// Number of events in the backlog.
const NOTIFIER_CAPACITY: usize = u16::MAX as usize;

type EvmTask = (EvmInput, oneshot::Sender<anyhow::Result<TransactionExecution>>);

/// The EthExecutor struct is responsible for orchestrating the execution of Ethereum transactions.
/// It holds references to the EVM, block miner, and storage, managing the overall process of
/// transaction execution, block production, and state management.
pub struct EthExecutor {
    // Tokio runtime where background EVMs are executed (keep reference because it cannot be dropped).
    _runtime_guard: Runtime,

    // Channel to send transactions to background EVMs.
    evm_tx: async_channel::Sender<EvmTask>,

    // Mutex-wrapped miner for creating new blockchain blocks.
    miner: Mutex<BlockMiner>,

    // Shared storage backend for persisting blockchain state.
    eth_storage: Arc<dyn EthStorage>,

    // Broadcast channels for notifying subscribers about new blocks and logs.
    block_notifier: broadcast::Sender<Block>,
    log_notifier: broadcast::Sender<LogMined>,
}

impl EthExecutor {
    /// Creates a new executor.
    pub fn new(evms: NonEmpty<Box<dyn Evm>>, eth_storage: Arc<dyn EthStorage>) -> Self {
        let (runtime, evm_tx) = spawn_background_evms(evms);

        Self {
            _runtime_guard: runtime,
            evm_tx,
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
            "executing real transaction"
        );

        // validate
        if transaction.signer.is_zero() {
            tracing::warn!("rejecting transaction from zero address");
            return Err(anyhow!("Transaction sent from zero address is not allowed."));
        }

        // execute transaction
        let execution = self.execute_in_evm(transaction.clone().try_into()?).await?;

        // mine block
        let mut miner_lock = self.miner.lock().await;
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
            "executing read-only transaction"
        );

        let execution = self.execute_in_evm((input, point_in_time).into()).await?;
        Ok(execution)
    }

    /// Submits a transaction to the EVM and awaits for its execution.
    async fn execute_in_evm(&self, evm_input: EvmInput) -> anyhow::Result<TransactionExecution> {
        let (execution_tx, execution_rx) = oneshot::channel::<anyhow::Result<TransactionExecution>>();
        self.evm_tx.send((evm_input, execution_tx)).await?;
        execution_rx.await?
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

fn spawn_background_evms(evms: NonEmpty<Box<dyn Evm>>) -> (Runtime, async_channel::Sender<EvmTask>) {
    // create tokio runtime to run evm
    let runtime = new_tokio_runtime("tokio-evm", 1, evms.len());

    // for each evm, spawn a new blocking task that runs in an infinite loop executing tasks
    let (evm_tx, evm_rx) = async_channel::unbounded::<EvmTask>();
    for mut evm in evms {
        let evm_rx = evm_rx.clone();

        runtime.spawn_blocking(move || loop {
            let (input, tx) = match evm_rx.recv_blocking() {
                Ok((input, tx)) => (input, tx),
                Err(e) => {
                    tracing::error!(reason = ?e, "failed to receive evm task");
                    continue;
                }
            };
            if let Err(e) = tx.send(evm.execute(input)) {
                tracing::error!(reason = ?e, "failed to send evm execution result");
            };
        });
    }

    (runtime, evm_tx)
}
