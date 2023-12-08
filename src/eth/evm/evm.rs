use crate::eth::primitives::Address;
use crate::eth::primitives::Bytecode;
use crate::eth::primitives::TransactionExecution;
use crate::eth::EthError;

/// EVM operations.
pub trait Evm: Send + Sync + 'static {
    // Execute a transaction that deploys a new contract.
    fn do_deployment(&mut self, input: EvmDeployment) -> Result<TransactionExecution, EthError>;

    /// Execute a transaction that calls a function of a deployed contract.
    fn do_transaction(&mut self, input: EvmTransaction) -> Result<TransactionExecution, EthError>;

    /// Execute the operation that saves the result of a transaction execution.
    fn do_save(&mut self, execution: TransactionExecution) -> Result<(), EthError>;

    /// Executes a contract deployment and return the address of the deployed contract.
    fn deploy(&mut self, input: EvmDeployment) -> Result<Address, EthError> {
        tracing::info!(caller = %input.caller, bytecode_len = input.data.len(), bytecode = %const_hex::encode_prefixed(&input.data), "deploying contract");

        // validate
        if input.caller == Address::ZERO {
            tracing::warn!("rejecting deployment from zero address");
            return Err(EthError::ZeroSigner);
        }

        // execute and save
        let execution = self.do_deployment(input)?;
        let deployment_address = execution.deployment_address();
        self.do_save(execution)?;

        // return deployed contract address
        match deployment_address {
            Some(address) => Ok(address),
            None => Err(EthError::DeploymentWithoutAddress),
        }
    }

    /// Execute a transaction, mutate the state and return function output.
    fn transact(&mut self, input: EvmTransaction) -> Result<(), EthError> {
        tracing::info!(caller = %input.caller, contract = %input.contract, data_len = %input.data.len(), "executing transaction");

        // validate
        if input.caller == Address::ZERO {
            tracing::warn!("rejecting transaction from zero address");
            return Err(EthError::ZeroSigner);
        }

        // execute and save
        let execution = self.do_transaction(input)?;
        self.do_save(execution)?;

        Ok(())
    }

    /// Execute a function and return the function output. State changes are ignored.
    /// TODO: return value
    fn call(&mut self, input: EvmCall) -> Result<(), EthError> {
        tracing::info!(contract = %input.contract, data_len = input.data.len(), "calling contract");

        // execute, but not save
        let input = EvmTransaction {
            caller: Address::ZERO,
            contract: input.contract,
            data: input.data,
        };
        let _ = self.do_transaction(input)?;

        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct EvmDeployment {
    pub caller: Address,
    pub data: Bytecode,
}

#[derive(Debug, Default)]
pub struct EvmTransaction {
    pub caller: Address,
    pub contract: Address,
    pub data: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct EvmCall {
    pub contract: Address,
    pub data: Vec<u8>,
}
