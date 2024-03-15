use async_trait::async_trait;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;

/// Temporary storage (in-between blocks) operations
#[async_trait]
pub trait TemporaryStorage: Send + Sync {
    /// Sets the block number being mined.
    async fn set_active_block_number(&self, number: BlockNumber) -> anyhow::Result<()>;

    // Retrieves the block number being mined.
    async fn read_active_block_number(&self) -> anyhow::Result<BlockNumber>;

    /// Retrieves an account from the storage. Returns Option when not found.
    async fn maybe_read_account(&self, address: &Address) -> anyhow::Result<Option<Account>>;

    /// Retrieves an slot from the storage. Returns Option when not found.
    async fn maybe_read_slot(&self, address: &Address, slot_index: &SlotIndex) -> anyhow::Result<Option<Slot>>;

    /// Temporarily stores account changes during block production
    async fn save_account_changes(&self, changes: Vec<ExecutionAccountChanges>) -> anyhow::Result<()>;

    /// Resets all state
    async fn reset(&self) -> anyhow::Result<()>;
}
