use async_trait::async_trait;

use super::EthStorageError;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Execution;
use crate::eth::primitives::ExecutionConflicts;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragePointInTime;

/// Temporary storage (in-between blocks) operations
// TODO: add Metrified method
#[async_trait]
pub trait TemporaryStorage: Send + Sync  {
    // Retrieves the last mined block number.
    async fn read_current_block_number(&self) -> anyhow::Result<BlockNumber>;

    /// Atomically increments the block number, returning the new value.
    async fn increment_block_number(&self) -> anyhow::Result<BlockNumber>;

    /// Checks if the transaction execution conflicts with the current storage state.
    async fn check_conflicts(&self, execution: &Execution) -> anyhow::Result<Option<ExecutionConflicts>>;

    /// Retrieves an account from the storage. Returns Option when not found.
    async fn maybe_read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>>;

    /// Retrieves an slot from the storage. Returns Option when not found.
    async fn maybe_read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>>;

    /// Commits changes to permanent storage
    async fn commit(&self, block: Block) -> anyhow::Result<(), EthStorageError>;

    /// Temporarily stores account changes during block production
    async fn save_account_changes(&self, block_number: BlockNumber, execution: Execution) -> anyhow::Result<()>;

    /// Resets all state
    async fn reset(&self) -> anyhow::Result<()>;
}
