use std::sync::Arc;
use std::sync::Mutex;

use crate::eth::evm::Evm;
use crate::eth::miner::BlockMiner;
use crate::eth::primitives::CallInput;
use crate::eth::primitives::StoragerPointInTime;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::eth::rpc::RpcBlockNotifier;
use crate::eth::rpc::RpcLogNotifier;
use crate::eth::storage::EthStorage;
use crate::eth::EthError;

/// High-level coordinator of Ethereum transactions.
pub struct EthExecutor {
    evm: Mutex<Box<dyn Evm>>,
    miner: Mutex<BlockMiner>,
    eth_storage: Arc<dyn EthStorage>,

    rpc_block_notifier: Option<RpcBlockNotifier>,
    rpc_log_notifier: Option<RpcLogNotifier>,
}

impl EthExecutor {
    /// Creates a new executor.
    pub fn new(evm: Box<impl Evm>, eth_storage: Arc<dyn EthStorage>) -> Self {
        Self {
            evm: Mutex::new(evm),
            miner: Mutex::new(BlockMiner::new(Arc::clone(&eth_storage))),
            eth_storage,
            rpc_block_notifier: None,
            rpc_log_notifier: None,
        }
    }

    /// Sets the RPC block notifier.
    pub fn set_rpc_block_notifier(&mut self, block_notifier: RpcBlockNotifier) {
        self.rpc_block_notifier = Some(block_notifier);
    }

    /// Sets the RPC log notifier.
    pub fn set_rpc_log_notifier(&mut self, log_notifier: RpcLogNotifier) {
        self.rpc_log_notifier = Some(log_notifier);
    }

    /// Execute a transaction, mutate the state and return function output.
    ///
    /// TODO: too much cloning that can be optimized here.
    pub fn transact(&self, transaction: TransactionInput) -> Result<TransactionExecution, EthError> {
        let signer = transaction.signer()?;

        tracing::info!(
            hash = %transaction.hash,
            nonce = %transaction.nonce,
            from = %transaction.from,
            signer = %signer,
            to = ?transaction.to,
            data_len = %transaction.input.len(),
            data = %transaction.input,
            "evm executing transaction"
        );

        // validate
        if signer.is_zero() {
            tracing::warn!("rejecting transaction from zero address");
            return Err(EthError::ZeroSigner);
        }

        // acquire locks
        let mut evm_lock = self.evm.lock().unwrap();
        let mut miner_lock = self.miner.lock().unwrap();

        // execute, mine and save
        let execution = evm_lock.execute(transaction.clone().try_into()?)?;
        let block = miner_lock.mine_with_one_transaction(signer, transaction, execution.clone())?;
        self.eth_storage.save_block(block.clone())?;

        // notify new blocks
        if let Some(block_notifier) = &self.rpc_block_notifier {
            if let Err(e) = block_notifier.send(block.clone()) {
                tracing::error!(reason = ?e, "failed to send block notification");
            };
        }

        // notify transaction logs
        if let Some(log_notifier) = &self.rpc_log_notifier {
            for trx in block.transactions {
                for log in trx.logs {
                    if let Err(e) = log_notifier.send(log) {
                        tracing::error!(reason = ?e, "failed to send log notification");
                    };
                }
            }
        }

        Ok(execution)
    }

    /// Execute a function and return the function output. State changes are ignored.
    /// TODO: return value
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
}
