use std::sync::Arc;
use std::sync::Mutex;

use tokio::sync::broadcast;

use super::primitives::LogMined;
use crate::eth::evm::Evm;
use crate::eth::miner::BlockMiner;
use crate::eth::primitives::Block;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::StoragerPointInTime;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::eth::storage::EthStorage;
use crate::eth::EthError;

/// High-level coordinator of Ethereum transactions.
pub struct EthExecutor {
    evm: Mutex<Box<dyn Evm>>,
    miner: Mutex<BlockMiner>,
    eth_storage: Arc<dyn EthStorage>,

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

    /// Execute a transaction, mutate the state and return function output.
    ///
    /// TODO: too much cloning that can be optimized here.
    pub fn transact(&self, transaction: TransactionInput) -> Result<TransactionExecution, EthError> {
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
            return Err(EthError::ZeroSigner);
        }

        // acquire locks
        let mut evm_lock = self.evm.lock().unwrap();
        let mut miner_lock = self.miner.lock().unwrap();

        // execute, mine and save
        let execution = evm_lock.execute(transaction.clone().try_into()?)?;
        let block = miner_lock.mine_with_one_transaction(transaction, execution.clone())?;
        self.eth_storage.save_block(block.clone())?;

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
    pub fn call(&self, input: CallInput, point_in_time: StoragerPointInTime) -> Result<TransactionExecution, EthError> {
        tracing::info!(
            from = %input.from,
            to = ?input.to,
            data_len = input.data.len(),
            data = %input.data,
            "evm executing call"
        );

        // execute, but not save
        let mut executor_lock = self.evm.lock().unwrap();
        let execution = executor_lock.execute((input, point_in_time).into())?;
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
