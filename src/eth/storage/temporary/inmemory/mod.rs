//! In-memory storage implementations.

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
#[cfg(feature = "dev")]
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::Hash;
#[cfg(feature = "dev")]
use crate::eth::primitives::Nonce;
use crate::eth::primitives::PendingBlock;
use crate::eth::primitives::PendingBlockHeader;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StorageError;
use crate::eth::primitives::TransactionExecution;
#[cfg(feature = "dev")]
use crate::eth::primitives::Wei;
use crate::eth::storage::ReadKind;
use crate::eth::storage::TxCount;
use crate::eth::storage::temporary::inmemory::call::InMemoryCallTemporaryStorage;
use crate::eth::storage::temporary::inmemory::transaction::InmemoryTransactionTemporaryStorage;

mod call;
mod transaction;

#[derive(Debug)]
pub struct InMemoryTemporaryStorage {
    pub transaction_storage: InmemoryTransactionTemporaryStorage,
    pub call_storage: InMemoryCallTemporaryStorage,
}

impl InMemoryTemporaryStorage {
    pub fn new(block_number: BlockNumber) -> Self {
        Self {
            transaction_storage: InmemoryTransactionTemporaryStorage::new(block_number),
            call_storage: InMemoryCallTemporaryStorage::new(),
        }
    }

    pub fn read_pending_block_header(&self) -> (PendingBlockHeader, TxCount) {
        self.transaction_storage.read_pending_block_header()
    }

    #[cfg(feature = "dev")]
    pub fn set_pending_block_header(&self, block_number: BlockNumber) -> anyhow::Result<(), StorageError> {
        self.transaction_storage.set_pending_block_header(block_number)
    }

    pub fn set_pending_from_external(&self, block: &ExternalBlock) {
        self.transaction_storage.set_pending_from_external(block);
    }

    pub fn save_pending_execution(&self, tx: TransactionExecution, is_local: bool) -> Result<(), StorageError> {
        self.call_storage.update_state_with_transaction(&tx);
        self.transaction_storage.save_pending_execution(tx, is_local)
    }

    pub fn read_pending_executions(&self) -> Vec<TransactionExecution> {
        self.transaction_storage.read_pending_executions()
    }

    pub fn finish_pending_block(&self) -> anyhow::Result<(PendingBlock, ExecutionChanges), StorageError> {
        self.call_storage.retain_recent_blocks();
        self.transaction_storage.finish_pending_block()
    }

    pub fn read_pending_execution(&self, hash: Hash) -> anyhow::Result<Option<TransactionExecution>, StorageError> {
        self.transaction_storage.read_pending_execution(hash)
    }

    pub fn read_account(&self, address: Address, kind: ReadKind) -> anyhow::Result<Option<Account>, StorageError> {
        match kind {
            ReadKind::Call((block_number, tx_count)) => Ok(self.call_storage.read_account(block_number, tx_count, address)),
            _ => self.transaction_storage.read_account(address),
        }
    }

    pub fn read_slot(&self, address: Address, index: SlotIndex, kind: ReadKind) -> anyhow::Result<Option<Slot>, StorageError> {
        match kind {
            ReadKind::Call((block_number, tx_count)) => Ok(self.call_storage.read_slot(block_number, tx_count, address, index)),
            _ => self.transaction_storage.read_slot(address, index),
        }
    }

    #[cfg(feature = "dev")]
    pub fn save_slot(&self, address: Address, slot: Slot) -> anyhow::Result<(), StorageError> {
        self.transaction_storage.save_slot(address, slot)
    }

    #[cfg(feature = "dev")]
    pub fn save_account_nonce(&self, address: Address, nonce: Nonce) -> anyhow::Result<(), StorageError> {
        self.transaction_storage.save_account_nonce(address, nonce)
    }

    #[cfg(feature = "dev")]
    pub fn save_account_balance(&self, address: Address, balance: Wei) -> anyhow::Result<(), StorageError> {
        self.transaction_storage.save_account_balance(address, balance)
    }

    #[cfg(feature = "dev")]
    pub fn save_account_code(&self, address: Address, code: Bytes) -> anyhow::Result<(), StorageError> {
        self.transaction_storage.save_account_code(address, code)
    }

    pub fn reset(&self) -> anyhow::Result<(), StorageError> {
        self.call_storage.reset();
        self.transaction_storage.reset()
    }
}

// -----------------------------------------------------------------------------
// Inner State
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct InMemoryTemporaryStorageState {
    /// Block that is being mined.
    pub block: PendingBlock,

    /// Last state of accounts and slots. Can be recreated from the executions inside the pending block.
    pub accounts: ExecutionChanges,
}

impl InMemoryTemporaryStorageState {
    pub fn new(block_number: BlockNumber) -> Self {
        Self {
            block: PendingBlock::new_at_now(block_number),
            accounts: ExecutionChanges::new(),
        }
    }

    pub fn reset(&mut self) {
        self.block = PendingBlock::new_at_now(1.into());
        self.accounts.clear();
    }
}
