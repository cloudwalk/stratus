use std::sync::Arc;
use std::sync::Mutex;

use crate::eth::evm::Evm;
use crate::eth::miner::BlockMiner;
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
}

impl EthExecutor {
    pub fn new(evm: Box<impl Evm>, eth_storage: Arc<dyn EthStorage>) -> Self {
        Self {
            evm: Mutex::new(evm),
            miner: Mutex::new(BlockMiner::new(Arc::clone(&eth_storage))),
            eth_storage,
        }
    }

    /// Execute a transaction, mutate the state and return function output.
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
        if execution.is_success() {
            let block = miner_lock.mine_with_one_transaction(signer, transaction, execution.clone())?;
            self.eth_storage.save_block(block)?;
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
