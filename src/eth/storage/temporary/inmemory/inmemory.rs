//! In-memory storage implementations.

use std::collections::HashMap;

use super::pending::InmemoryPendingTemporaryStorage;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
#[cfg(feature = "dev")]
use crate::eth::primitives::Bytes;
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
use crate::eth::primitives::UnixTime;
#[cfg(feature = "dev")]
use crate::eth::primitives::UnixTimeNow;
#[cfg(feature = "dev")]
use crate::eth::primitives::Wei;
use crate::eth::storage::AccountWithSlots;

#[derive(Debug)]
pub struct InMemoryTemporaryStorage {
    pub pending_storage: InmemoryPendingTemporaryStorage,
}

impl InMemoryTemporaryStorage {
    pub fn new(block_number: BlockNumber) -> Self {
        Self {
            pending_storage: InmemoryPendingTemporaryStorage::new(block_number),
        }
    }

    pub fn read_pending_block_header(&self) -> PendingBlockHeader {
        self.pending_storage.read_pending_block_header()
    }

    #[cfg(feature = "dev")]
    pub fn set_pending_block_header(&self, block_number: BlockNumber) -> anyhow::Result<(), StorageError> {
        self.pending_storage.set_pending_block_header(block_number)
    }

    pub fn save_pending_execution(&self, tx: TransactionExecution, check_conflicts: bool, is_local: bool) -> Result<(), StorageError> {
        self.pending_storage.save_pending_execution(tx, check_conflicts, is_local)
    }

    pub fn read_pending_executions(&self) -> Vec<TransactionExecution> {
        self.pending_storage.read_pending_executions()
    }

    pub fn finish_pending_block(&self) -> anyhow::Result<PendingBlock, StorageError> {
        self.pending_storage.finish_pending_block()
    }

    pub fn read_pending_execution(&self, hash: Hash) -> anyhow::Result<Option<TransactionExecution>, StorageError> {
        self.pending_storage.read_pending_execution(hash)
    }

    pub fn read_account(&self, address: Address) -> anyhow::Result<Option<Account>, StorageError> {
        self.pending_storage.read_account(address)
    }

    pub fn read_slot(&self, address: Address, index: SlotIndex) -> anyhow::Result<Option<Slot>, StorageError> {
        self.pending_storage.read_slot(address, index)
    }

    #[cfg(feature = "dev")]
    pub fn save_slot(&self, address: Address, slot: Slot) -> anyhow::Result<(), StorageError> {
        self.pending_storage.save_slot(address, slot)
    }

    #[cfg(feature = "dev")]
    pub fn save_account_nonce(&self, address: Address, nonce: Nonce) -> anyhow::Result<(), StorageError> {
        self.pending_storage.save_account_nonce(address, nonce)
    }

    #[cfg(feature = "dev")]
    pub fn save_account_balance(&self, address: Address, balance: Wei) -> anyhow::Result<(), StorageError> {
        self.pending_storage.save_account_balance(address, balance)
    }

    #[cfg(feature = "dev")]
    pub fn save_account_code(&self, address: Address, code: Bytes) -> anyhow::Result<(), StorageError> {
        self.pending_storage.save_account_code(address, code)
    }

    pub fn reset(&self) -> anyhow::Result<(), StorageError> {
        self.pending_storage.reset()
    }
}

// -----------------------------------------------------------------------------
// Inner State
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct InMemoryTemporaryStorageState {
    /// Block that is being mined.
    pub block: PendingBlock,

    /// Last state of accounts and slots. Can be recreated from the executions inside the pending block.
    pub accounts: HashMap<Address, AccountWithSlots, hash_hasher::HashBuildHasher>,
}

impl InMemoryTemporaryStorageState {
    pub fn new(block_number: BlockNumber) -> Self {
        Self {
            block: PendingBlock::new_at_now(block_number),
            accounts: HashMap::default(),
        }
    }

    pub fn reset(&mut self) {
        self.block = PendingBlock::new_at_now(1.into());
        self.accounts.clear();
    }
}
