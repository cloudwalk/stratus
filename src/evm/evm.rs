use std::marker::PhantomData;

use super::entities::Address;
use super::entities::Bytecode;
use super::entities::TransactionExecution;
use super::EvmError;

/// EVM operations.
///
/// Implementation based on [Template Method pattern](https://refactoring.guru/design-patterns/template-method/rust/example).
pub trait Evm: Sized {
    // -------------------------------------------------------------------------
    // Individual steps (should be implemented, but not called directly).
    // -------------------------------------------------------------------------

    // Execute a transaction that deploys a new contract.
    fn _do_deployment(&mut self, caller: &Address, bytecode: &Bytecode) -> Result<TransactionExecution, EvmError>;

    /// Execute a transaction that calls a function of a deployed contract.
    fn _do_transaction(&mut self, caller: &Address, contract: &Address, data: &[u8]) -> Result<TransactionExecution, EvmError>;

    /// Execute the operation that saves the result of a transaction execution.
    fn _do_save(&mut self, execution: &TransactionExecution) -> Result<(), EvmError>;

    // -------------------------------------------------------------------------
    // Public interface that can be called by clients.
    // -------------------------------------------------------------------------

    /// Returns a builder that can be used to execute EVM transactions or deployments.
    fn from<'a>(&'a mut self, address: &'a Address) -> EvmAction<Self, DeployContract> {
        EvmAction {
            evm: self,
            from: address,
            to: &Address::ZERO,
            allow_from_zero: false,
            phantom: Default::default(),
        }
    }

    /// Returns a builder that can be used to execute EVM calls.
    fn to<'a>(&'a mut self, address: &'a Address) -> EvmAction<Self, ReadContract> {
        EvmAction {
            evm: self,
            from: &Address::ZERO,
            to: address,
            allow_from_zero: false,
            phantom: Default::default(),
        }
    }
}

// -----------------------------------------------------------------------------
// EVM - Contract deployment
// -----------------------------------------------------------------------------

pub trait EvmActionState {}

pub struct ReadContract;
impl EvmActionState for ReadContract {}

pub struct WriteContract;
impl EvmActionState for WriteContract {}

pub struct DeployContract;
impl EvmActionState for DeployContract {}

pub struct EvmAction<'a, EVM, S>
where
    EVM: Evm,
    S: EvmActionState,
{
    evm: &'a mut EVM,
    from: &'a Address,
    to: &'a Address,
    allow_from_zero: bool,
    phantom: PhantomData<S>,
}

impl<'a, T> EvmAction<'a, T, DeployContract>
where
    T: Evm,
{
    pub fn to(self, address: &'a Address) -> EvmAction<'_, T, WriteContract> {
        EvmAction {
            evm: self.evm,
            from: self.from,
            to: address,
            allow_from_zero: self.allow_from_zero,
            phantom: Default::default(),
        }
    }

    /// Executes a contract deployment and return the address of the deployed contract.
    pub fn deploy(self, bytecode: impl Into<Bytecode>) -> Result<Address, EvmError> {
        if self.from == &Address::ZERO {
            return Err(EvmError::TransactionFromZeroAddress);
        }

        let execution = self.evm._do_deployment(self.from, &bytecode.into())?;
        self.evm._do_save(&execution)?;

        match execution.deployment_address() {
            Some(address) => Ok(address),
            None => Err(EvmError::ContractDeploymentWithoutAddress),
        }
    }
}

impl<'a, T> EvmAction<'a, T, WriteContract>
where
    T: Evm,
{
    /// Allow transaction to be sent from zero address.
    pub fn allow_from_zero(mut self) -> Self {
        self.allow_from_zero = true;
        self
    }

    /// Execute a transaction, mutate the state and return function output.
    pub fn write(self, data: &[u8]) -> Result<(), EvmError> {
        if self.from == &Address::ZERO && !self.allow_from_zero {
            return Err(EvmError::TransactionFromZeroAddress);
        }

        let execution = self.evm._do_transaction(self.from, self.to, data)?;
        self.evm._do_save(&execution)?;
        Ok(())
    }
}

impl<'a, T> EvmAction<'a, T, ReadContract>
where
    T: Evm,
{
    /// Execute a function and return the function output. State changes are ignored.
    /// TODO: return value
    pub fn read(self, data: &[u8]) -> Result<(), EvmError> {
        let _ = self.evm._do_transaction(self.from, self.to, data)?;
        Ok(())
    }
}
