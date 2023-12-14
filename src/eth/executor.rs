use std::sync::Arc;
use std::sync::Mutex;

use crate::eth::evm::Evm;
use crate::eth::miner::BlockMiner;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::TransactionInput;
use crate::eth::storage::BlockNumberStorage;
use crate::eth::storage::EthStorage;
use crate::eth::EthError;

/// High-level coordinator of Ethereum transactions.
pub struct EthExecutor {
    evm: Mutex<Box<dyn Evm>>,
    miner: Mutex<BlockMiner>,
    eth_storage: Arc<dyn EthStorage>,
}

impl EthExecutor {
    pub fn new(evm: Box<impl Evm>, eth_storage: Arc<impl EthStorage>, block_number_storage: Arc<impl BlockNumberStorage>) -> Self {
        Self {
            evm: Mutex::new(evm),
            miner: Mutex::new(BlockMiner::new(block_number_storage)),
            eth_storage,
        }
    }

    /// Executes a contract deployment and return the address of the deployed contract.
    pub fn deploy(&self, input: EthDeployment) -> Result<Address, EthError> {
        tracing::info!(hash = %input.transaction.hash, caller = %input.caller, bytecode_len = input.data.len(), "deploying contract");

        // validate
        if input.caller.is_zero() {
            tracing::warn!("rejecting deployment from zero address");
            return Err(EthError::ZeroSigner);
        }

        // acquire locks
        let mut evm_lock = self.evm.lock().unwrap();
        let mut miner_lock = self.miner.lock().unwrap();

        // execute, mine and save
        let execution = evm_lock.transact(input.clone().into())?;
        let deployment_address = execution.contract_address();
        let block = miner_lock.mine_one_transaction(input.transaction, execution)?;
        self.eth_storage.save_block(block)?;

        // return deployed contract address
        match deployment_address {
            Some(address) => Ok(address),
            None => Err(EthError::DeploymentWithoutAddress),
        }
    }

    /// Execute a transaction, mutate the state and return function output.
    pub fn transact(&self, input: EthTransaction) -> Result<(), EthError> {
        tracing::info!(hash = %input.transaction.hash, caller = %input.caller, contract = %input.contract, data_len = %input.data.len(), data = %input.data, "executing transaction");

        // validate
        if input.caller.is_zero() {
            tracing::warn!("rejecting transaction from zero address");
            return Err(EthError::ZeroSigner);
        }

        // acquire locks
        let mut evm_lock = self.evm.lock().unwrap();
        let mut miner_lock = self.miner.lock().unwrap();

        // execute, mine and save
        let execution = evm_lock.transact(input.clone().into())?;
        let block = miner_lock.mine_one_transaction(input.transaction, execution)?;
        self.eth_storage.save_block(block)?;

        Ok(())
    }

    /// Execute a function and return the function output. State changes are ignored.
    /// TODO: return value
    pub fn call(&self, input: EthCall) -> Result<Bytes, EthError> {
        tracing::info!(contract = %input.contract, data_len = input.data.len(), data = %input.data, "calling contract");

        // execute, but not save
        let mut executor_lock = self.evm.lock().unwrap();
        let result = executor_lock.transact(input.into())?;
        Ok(result.output)
    }
}

#[derive(Debug, Clone, Default)]
pub struct EthDeployment {
    pub transaction: TransactionInput,
    pub caller: Address,
    pub data: Bytes,
}

#[derive(Debug, Clone, Default)]
pub struct EthTransaction {
    pub transaction: TransactionInput,
    pub caller: Address,
    pub contract: Address,
    pub data: Bytes,
}

#[derive(Debug, Clone, Default)]
pub struct EthCall {
    pub contract: Address,
    pub data: Bytes,
}
