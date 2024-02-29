use async_trait::async_trait;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotSample;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::StorageError;

/// Permanent (committed) storage operations
#[async_trait]
pub trait PermanentStorage: Send + Sync {
    // Retrieves the last mined block number.
    async fn read_current_block_number(&self) -> anyhow::Result<BlockNumber>;

    /// Atomically increments the block number, returning the new value.
    async fn increment_block_number(&self) -> anyhow::Result<BlockNumber>;

    /// Sets the block number to a specific value.
    ///
    /// Should be used only when importing external blocks. To reset the storage to the past, use `reset_at`.
    async fn set_block_number(&self, number: BlockNumber) -> anyhow::Result<()>;

    /// Retrieves an account from the storage. Returns Option when not found.
    async fn maybe_read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>>;

    /// Retrieves an slot from the storage. Returns Option when not found.
    async fn maybe_read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>>;

    /// Retrieves a block from the storage.
    async fn read_block(&self, block_selection: &BlockSelection) -> anyhow::Result<Option<Block>>;

    /// Retrieves a transaction from the storage.
    async fn read_mined_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>>;

    /// Retrieves logs from the storage.
    async fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>>;

    /// Persists atomically all changes from a block.
    async fn save_block(&self, block: Block) -> anyhow::Result<(), StorageError>;

    /// Persists initial accounts (test accounts or genesis accounts).
    async fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()>;

    /// Resets all state to a specific block number.
    async fn reset_at(&self, number: BlockNumber) -> anyhow::Result<()>;

    async fn read_slots_sample(&self, start: BlockNumber, end: BlockNumber, max_samples: u64, seed: u64) -> anyhow::Result<Vec<SlotSample>>;
}
