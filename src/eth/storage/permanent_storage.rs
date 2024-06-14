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

/// Permanent (committed) storage operations
#[async_trait]
pub trait PermanentStorage: Send + Sync {
    // -------------------------------------------------------------------------
    // Block number
    // -------------------------------------------------------------------------

    /// Sets the last mined block number to a specific value.
    async fn set_mined_block_number(&self, number: BlockNumber) -> anyhow::Result<()>;

    // Retrieves the last mined block number.
    async fn read_mined_block_number(&self) -> anyhow::Result<BlockNumber>;

    // -------------------------------------------------------------------------
    // Block
    // -------------------------------------------------------------------------

    /// Persists atomically all changes from a block.
    async fn save_block(&self, block: Block) -> anyhow::Result<()>;

    /// Retrieves a block from the storage.
    async fn read_block(&self, block_selection: &BlockSelection) -> anyhow::Result<Option<Block>>;

    /// Retrieves a transaction from the storage.
    async fn read_mined_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>>;

    /// Retrieves logs from the storage.
    async fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>>;

    // -------------------------------------------------------------------------
    // Account and slots
    // -------------------------------------------------------------------------

    /// Persists initial accounts (test accounts or genesis accounts).
    async fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()>;

    /// Retrieves an account from the storage. Returns Option when not found.
    async fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>>;

    /// Retrieves an slot from the storage. Returns Option when not found.
    async fn read_slot(&self, address: &Address, index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>>;

    /// Retrieves a random sample of slots, from the provided start and end blocks.
    async fn read_slots_sample(&self, start: BlockNumber, end: BlockNumber, max_samples: u64, seed: u64) -> anyhow::Result<Vec<SlotSample>>;

    /// Retrieves all current slots associated to an address.
    async fn read_all_slots(&self, address: &Address) -> anyhow::Result<Vec<Slot>>;

    // -------------------------------------------------------------------------
    // Global state
    // -------------------------------------------------------------------------

    /// Resets all state to a specific block number.
    async fn reset_at(&self, number: BlockNumber) -> anyhow::Result<()>;
}
