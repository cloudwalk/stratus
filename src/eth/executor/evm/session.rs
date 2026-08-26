use std::sync::Arc;

use anyhow::anyhow;
use revm::Database;
use revm::DatabaseRef;
use revm::primitives::B256;
use revm::primitives::U256;
use revm::state::AccountInfo;

use crate::alias::RevmAddress;
use crate::alias::RevmBytecode;
use crate::eth::executor::evm::types::SlotAccessMetrics;
use crate::eth::storage::ExecutionKind;
use crate::eth::storage::StratusStorage;
use crate::eth::types::Address;
use crate::eth::types::SlotIndex;
use crate::eth::types::StratusError;
use crate::infra::metrics;

/// Contextual data that is read or set durint the execution of a transaction in the EVM.
pub struct RevmSession {
    /// Service to communicate with the storage.
    pub storage: Arc<StratusStorage>,

    /// Input passed to EVM to execute the transaction.
    pub kind: ExecutionKind,

    /// Metrics collected during EVM execution.
    pub metrics: SlotAccessMetrics,
}

impl RevmSession {
    /// Creates the base session to be used with REVM.
    pub fn new(storage: Arc<StratusStorage>) -> Self {
        Self {
            storage,
            kind: ExecutionKind::default(),
            metrics: SlotAccessMetrics::default(),
        }
    }

    /// Resets the session to be used with a new transaction.
    pub fn reset(&mut self, kind: ExecutionKind) {
        self.kind = kind;
        self.metrics = SlotAccessMetrics::default();
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
        let point_in_time = self.kind.point_in_time();
        let start = metrics::now();

        // retrieve account
        let address: Address = address.into();
        let (account, found_at) = self.storage.read_account(address, self.kind)?;
        metrics::inc_evm_basic_ref(start.elapsed(), point_in_time, found_at);
        Ok(Some(account.into()))
    }

    fn storage_ref(&self, address: revm::primitives::Address, index: U256) -> Result<U256, Self::Error> {
        let point_in_time = self.kind.point_in_time();
        let start = metrics::now();

        // convert slot
        let address: Address = address.into();
        let index: SlotIndex = index.into();

        // load slot from storage
        let (slot, found_at) = self.storage.read_slot(address, index, self.kind)?;
        metrics::inc_evm_storage_ref(start.elapsed(), point_in_time, found_at);

        Ok(slot.value.into())
    }

    fn block_hash_ref(&self, _: u64) -> Result<B256, Self::Error> {
        Err(anyhow!("block hash opcode not implemented").into())
    }

    fn code_by_hash_ref(&self, _code_hash: B256) -> Result<revm::state::Bytecode, Self::Error> {
        Err(anyhow!("code by hash opcode not implemented").into())
    }
}
