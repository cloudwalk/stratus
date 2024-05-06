use async_trait::async_trait;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Execution;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::TransactionKind;

/// Temporary storage (in-between blocks) operations
#[async_trait]
pub trait TemporaryStorage: Send + Sync {
    /// Sets the block number activelly being mined.
    async fn set_active_block_number(&self, number: BlockNumber) -> anyhow::Result<()>;

    // Retrieves the block number activelly being mined.
    async fn read_active_block_number(&self) -> anyhow::Result<Option<BlockNumber>>;

    /// Retrieves an account from the storage. Returns Option when not found.
    async fn read_account(&self, address: &Address) -> anyhow::Result<Option<Account>>;

    /// Retrieves an slot from the storage. Returns Option when not found.
    async fn read_slot(&self, address: &Address, index: &SlotIndex) -> anyhow::Result<Option<Slot>>;

    /// Saves a transaction and its execution result.
    async fn save_execution(&self, transaction: TransactionKind, execution: Execution) -> anyhow::Result<()>;

    /// If necessary, flushes temporary state to durable storage.
    async fn flush(&self) -> anyhow::Result<()>;

    /// Resets to default empty state.
    async fn reset(&self) -> anyhow::Result<()>;
}
