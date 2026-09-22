use std::sync::Arc;
use std::time::Instant;

use alloy_primitives::Uint;
use anyhow::anyhow;
use revm::Database;
use revm::DatabaseRef;
use revm::primitives::B256;
use revm::primitives::U256;
use revm::state::AccountInfo;

use crate::alias::RevmAddress;
use crate::alias::RevmBytecode;
use crate::eth::executor::evm::types::StorageMetrics;
use crate::eth::storage::ExecutionKind;
use crate::eth::storage::FoundAt;
use crate::eth::storage::StorageError;
use crate::eth::storage::StratusStorage;
use crate::eth::types::Address;
use crate::eth::types::SlotIndex;
use crate::eth::types::StratusError;

/// Contextual data that is read or set durint the execution of a transaction in the EVM.
pub struct RevmSession {
    /// Service to communicate with the storage.
    pub storage: Arc<StratusStorage>,

    /// Input passed to EVM to execute the transaction.
    pub kind: ExecutionKind,

    /// Metrics collected during EVM execution.
    pub metrics: StorageMetrics,
}

impl RevmSession {
    /// Creates the base session to be used with REVM.
    pub fn new(storage: Arc<StratusStorage>) -> Self {
        Self {
            storage,
            kind: ExecutionKind::default(),
            metrics: StorageMetrics::default(),
        }
    }

    /// Resets the session to be used with a new transaction.
    pub fn reset(&mut self, kind: ExecutionKind) {
        self.kind = kind;
        self.metrics = StorageMetrics::default();
    }
}

impl Database for RevmSession {
    type Error = StratusError;

    fn basic(&mut self, revm_address: RevmAddress) -> Result<Option<AccountInfo>, StratusError> {
        let start = Instant::now();
        let (account, found_at) = self.read_account(revm_address)?;
        self.metrics.account_reads.record(found_at, start.elapsed());
        Ok(account)
    }

    fn storage(&mut self, revm_address: RevmAddress, revm_index: U256) -> Result<U256, StratusError> {
        let start = Instant::now();
        let (slot, found_at) = self.read_slot(revm_address, revm_index)?;
        self.metrics.slot_reads.record(found_at, start.elapsed());
        Ok(slot)
    }

    fn code_by_hash(&mut self, _: B256) -> Result<RevmBytecode, StratusError> {
        Err(anyhow!("code by hash opcode not implemented").into())
    }

    fn block_hash(&mut self, _: u64) -> Result<B256, StratusError> {
        Err(anyhow!("block hash opcode not implemented").into())
    }
}

impl DatabaseRef for RevmSession {
    type Error = StratusError;

    fn basic_ref(&self, address: revm::primitives::Address) -> Result<Option<AccountInfo>, Self::Error> {
        Ok(self.read_account(address)?.0)
    }

    fn storage_ref(&self, address: revm::primitives::Address, index: U256) -> Result<U256, Self::Error> {
        Ok(self.read_slot(address, index)?.0)
    }

    fn block_hash_ref(&self, _: u64) -> Result<B256, Self::Error> {
        Err(anyhow!("block hash opcode not implemented").into())
    }

    fn code_by_hash_ref(&self, _code_hash: B256) -> Result<revm::state::Bytecode, Self::Error> {
        Err(anyhow!("code by hash opcode not implemented").into())
    }
}

impl RevmSession {
    pub fn read_account(&self, address: revm::primitives::Address) -> Result<(Option<AccountInfo>, FoundAt), StorageError> {
        let address: Address = address.into();

        if address.is_ignored() {
            return Ok((None, FoundAt::Temp));
        }

        let (account, found_at) = self.storage.read_account(address, self.kind)?;
        Ok((Some(account.into()), found_at))
    }

    pub fn read_slot(&self, address: revm::primitives::Address, index: U256) -> Result<(U256, FoundAt), StorageError> {
        let address: Address = address.into();

        if address.is_ignored() {
            return Ok((Uint::default(), FoundAt::Temp));
        }

        let index: SlotIndex = index.into();

        // load slot from storage
        let (slot, found_at) = self.storage.read_slot(address, index, self.kind)?;

        Ok((slot.value.into(), found_at))
    }
}
