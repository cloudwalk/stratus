use std::sync::Arc;
use std::sync::RwLock;

use crate::eth::evm::Evm;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytecode;
use crate::eth::primitives::Transaction;
use crate::eth::storage::EthStorage;
use crate::eth::EthError;

/// High-level coordinator of Ethereum transactions.
pub struct EthExecutor {
    evm: RwLock<Box<dyn Evm>>,
    storage: Arc<dyn EthStorage>,
}

impl EthExecutor {
    pub fn new(evm: Box<impl Evm>, storage: Arc<impl EthStorage>) -> Self {
        Self {
            evm: RwLock::new(evm),
            storage,
        }
    }

    /// Executes a contract deployment and return the address of the deployed contract.
    pub fn deploy(&self, input: EthDeployment) -> Result<Address, EthError> {
        tracing::info!(hash = %input.transaction.hash(), caller = %input.caller, bytecode_len = input.data.len(), "deploying contract");

        // validate
        if input.caller == Address::ZERO {
            tracing::warn!("rejecting deployment from zero address");
            return Err(EthError::ZeroSigner);
        }

        // execute and save
        let mut lock = self.evm.write().unwrap();
        let execution = lock.transact(input.clone().into())?;
        let deployment_address = execution.deployment_address();

        self.storage.save_execution(input.transaction, execution)?;

        // return deployed contract address
        match deployment_address {
            Some(address) => Ok(address),
            None => Err(EthError::DeploymentWithoutAddress),
        }
    }

    /// Execute a transaction, mutate the state and return function output.
    pub fn transact(&self, input: EthTransaction) -> Result<(), EthError> {
        tracing::info!(caller = %input.caller, contract = %input.contract, data_len = %input.data.len(), "executing transaction");

        // validate
        if input.caller == Address::ZERO {
            tracing::warn!("rejecting transaction from zero address");
            return Err(EthError::ZeroSigner);
        }

        // execute and save
        let mut lock = self.evm.write().unwrap();
        let execution = lock.transact(input.clone().into())?;
        self.storage.save_execution(input.transaction, execution)?;

        Ok(())
    }

    /// Execute a function and return the function output. State changes are ignored.
    /// TODO: return value
    pub fn call(&self, input: EthCall) -> Result<(), EthError> {
        tracing::info!(contract = %input.contract, data_len = input.data.len(), "calling contract");

        // execute, but not save
        let mut lock = self.evm.write().unwrap();
        let _ = lock.transact(input.into())?;
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct EthDeployment {
    pub transaction: Transaction,
    pub caller: Address,
    pub data: Bytecode,
}

#[derive(Debug, Clone, Default)]
pub struct EthTransaction {
    pub transaction: Transaction,
    pub caller: Address,
    pub contract: Address,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct EthCall {
    pub contract: Address,
    pub data: Vec<u8>,
}
