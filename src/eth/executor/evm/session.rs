use std::sync::Arc;

use anyhow::anyhow;
use revm::Database;
use revm::DatabaseRef;
use revm::primitives::B256;
use revm::primitives::U256;
use revm::state::AccountInfo;

use crate::alias::RevmAddress;
use crate::alias::RevmBytecode;
use crate::eth::executor::ExecutorConfig;
use crate::eth::executor::evm::types::EvmInput;
use crate::eth::primitives::Address;
use crate::eth::primitives::EvmExecutionMetrics;
use crate::eth::primitives::ExecutorError;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StratusError;
use crate::eth::storage::ReadKind;
use crate::eth::storage::StratusStorage;

/// Contextual data that is read or set durint the execution of a transaction in the EVM.
pub struct RevmSession {
    /// Executor configuration.
    config: ExecutorConfig,

    /// Service to communicate with the storage.
    pub storage: Arc<StratusStorage>,

    /// Input passed to EVM to execute the transaction.
    pub read_kind: ReadKind,

    /// Metrics collected during EVM execution.
    pub metrics: EvmExecutionMetrics,
}

impl RevmSession {
    /// Creates the base session to be used with REVM.
    pub fn new(storage: Arc<StratusStorage>, config: ExecutorConfig) -> Self {
        Self {
            config,
            storage,
            read_kind: ReadKind::default(),
            metrics: EvmExecutionMetrics::default(),
        }
    }

    /// Resets the session to be used with a new transaction.
    pub fn reset(&mut self, read_kind: ReadKind) {
        self.read_kind = read_kind;
        self.metrics = EvmExecutionMetrics::default();
    }

    /// Validates that the `to` account of the input is a contract when the input is a contract call.
    ///
    /// Reads the `to` account from storage upfront so that calls to non-contract accounts are rejected
    /// before EVM execution, rather than relying on `basic` being called for the `to` address during `transact`.
    pub fn validate_to_is_contract<Input: EvmInput>(&self, input: &Input) -> Result<(), StratusError> {
        if let Some(to_address) = input.to()
            && input.is_contract_call()
        {
            let account = self.storage.read_account(*to_address, input.kind())?;
            if account.bytecode.is_none() {
                if self.config.executor_reject_not_contract {
                    return Err(ExecutorError::AccountNotContract { address: *to_address }.into());
                } else {
                    tracing::warn!(%to_address, "evm to_account is not a contract because does not have bytecode");
                }
            }
        }
        Ok(())
    }
}

impl Database for RevmSession {
    type Error = StratusError;

    fn basic(&mut self, revm_address: RevmAddress) -> Result<Option<AccountInfo>, StratusError> {
        self.metrics.account_reads += 1;
        self.basic_ref(revm_address)
    }

    fn code_by_hash(&mut self, _: B256) -> Result<RevmBytecode, StratusError> {
        Err(anyhow!("code by hash opcode not implemented").into())
    }

    fn storage(&mut self, revm_address: RevmAddress, revm_index: U256) -> Result<U256, StratusError> {
        self.metrics.slot_reads += 1;
        self.storage_ref(revm_address, revm_index)
    }

    fn block_hash(&mut self, _: u64) -> Result<B256, StratusError> {
        Err(anyhow!("block hash opcode not implemented").into())
    }
}

impl DatabaseRef for RevmSession {
    type Error = StratusError;

    fn basic_ref(&self, address: revm::primitives::Address) -> Result<Option<AccountInfo>, Self::Error> {
        // retrieve account
        let address: Address = address.into();
        let account = self.storage.read_account(address, self.read_kind)?;
        Ok(Some(account.into()))
    }

    fn storage_ref(&self, address: revm::primitives::Address, index: U256) -> Result<U256, Self::Error> {
        // convert slot
        let address: Address = address.into();
        let index: SlotIndex = index.into();

        // load slot from storage
        let slot = self.storage.read_slot(address, index, self.read_kind)?;

        Ok(slot.value.into())
    }

    fn block_hash_ref(&self, _: u64) -> Result<B256, Self::Error> {
        Err(anyhow!("block hash opcode not implemented").into())
    }

    fn code_by_hash_ref(&self, _code_hash: B256) -> Result<revm::state::Bytecode, Self::Error> {
        Err(anyhow!("code by hash opcode not implemented").into())
    }
}
