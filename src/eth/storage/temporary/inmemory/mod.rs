//! In-memory storage implementations.

use crate::eth::executor::State;
use crate::eth::executor::TransactionExecution;
use crate::eth::executor::types::state::Complete;
use crate::eth::storage::ExecutionKind;
use crate::eth::storage::StorageError;
use crate::eth::storage::TxCount;
use crate::eth::storage::temporary::inmemory::call::InMemoryCallTemporaryStorage;
use crate::eth::storage::temporary::inmemory::transaction::InmemoryTransactionTemporaryStorage;
use crate::eth::types::Account;
use crate::eth::types::Address;
use crate::eth::types::BlockNumber;
#[cfg(feature = "dev")]
use crate::eth::types::Bytes;
use crate::eth::types::Hash;
#[cfg(feature = "dev")]
use crate::eth::types::Nonce;
use crate::eth::types::PendingBlock;
use crate::eth::types::PendingBlockHeader;
use crate::eth::types::Slot;
use crate::eth::types::SlotIndex;
use crate::eth::types::UnixTime;
#[cfg(feature = "dev")]
use crate::eth::types::Wei;

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

    pub fn set_pending_header(&self, number: BlockNumber, timestamp: UnixTime) {
        self.transaction_storage.set_pending_header(number, timestamp);
    }

    pub fn save_pending_execution(&self, tx: TransactionExecution) -> Result<(), StorageError> {
        self.call_storage.update_state_with_transaction(&tx);
        self.transaction_storage.save_pending_execution(tx)
    }

    pub fn read_pending_executions(&self) -> Vec<TransactionExecution> {
        self.transaction_storage.read_pending_executions()
    }

    pub fn finish_pending_block(&self) -> (PendingBlock, State<Complete>) {
        self.call_storage.retain_recent_blocks();
        self.transaction_storage.finish_pending_block()
    }

    pub fn read_pending_execution(&self, hash: Hash) -> anyhow::Result<Option<TransactionExecution>, StorageError> {
        self.transaction_storage.read_pending_execution(hash)
    }

    pub fn read_account(&self, address: Address, kind: ExecutionKind) -> Option<Account> {
        match kind {
            ExecutionKind::CallPending(block_number, tx_count) => self.call_storage.read_account(block_number, tx_count, address),
            ExecutionKind::CallLatest(block_number) => self.call_storage.read_account(block_number, TxCount::Full, address),
            ExecutionKind::CallPast(block_number) => self.call_storage.read_account(block_number, TxCount::Full, address),
            _ => self.transaction_storage.read_account(address),
        }
    }

    pub fn read_slot(&self, address: Address, index: SlotIndex, kind: ExecutionKind) -> Option<Slot> {
        match kind {
            ExecutionKind::CallPending(block_number, tx_count) => self.call_storage.read_slot(block_number, tx_count, address, index),
            ExecutionKind::CallLatest(block_number) => self.call_storage.read_slot(block_number, TxCount::Full, address, index),
            ExecutionKind::CallPast(block_number) => self.call_storage.read_slot(block_number, TxCount::Full, address, index),
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
    pub block_changes: State<Complete>,
}

impl InMemoryTemporaryStorageState {
    pub fn new(block_number: BlockNumber) -> Self {
        Self {
            block: PendingBlock::new_at_now(block_number),
            block_changes: State::default(),
        }
    }

    pub fn reset(&mut self) {
        self.block = PendingBlock::new_at_now(1.into());
        self.block_changes = State::default();
    }
}
