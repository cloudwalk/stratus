use async_trait::async_trait;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::TransactionExecution;

/// Temporary storage (in-between blocks) operations
#[async_trait]
pub trait TemporaryStorage: Send + Sync + TemporaryStorageExecutionOps {
    // -------------------------------------------------------------------------
    // Executions
    // -------------------------------------------------------------------------

    // -------------------------------------------------------------------------
    // Accounts and Slots
    // -------------------------------------------------------------------------

    /// Retrieves an account from the storage. Returns Option when not found.
    async fn read_account(&self, address: &Address) -> anyhow::Result<Option<Account>>;

    /// Retrieves an slot from the storage. Returns Option when not found.
    async fn read_slot(&self, address: &Address, index: &SlotIndex) -> anyhow::Result<Option<Slot>>;

    // -------------------------------------------------------------------------
    // Block number
    // -------------------------------------------------------------------------

    /// Sets the block number activelly being mined.
    async fn set_active_block_number(&self, number: BlockNumber) -> anyhow::Result<()>;

    // Retrieves the block number activelly being mined.
    async fn read_active_block_number(&self) -> anyhow::Result<Option<BlockNumber>>;

    // -------------------------------------------------------------------------
    // General state
    // -------------------------------------------------------------------------

    /// Resets to default empty state.
    async fn reset(&self) -> anyhow::Result<()>;

    /// If necessary, flushes temporary state to durable storage.
    async fn flush(&self) -> anyhow::Result<()>;
}

#[async_trait]
pub trait TemporaryStorageExecutionOps {
    /// Sets the external block being re-executed.
    async fn set_external_block(&self, block: ExternalBlock) -> anyhow::Result<()>;

    /// Reads an external block being re-executed.
    async fn read_external_block(&self) -> anyhow::Result<Option<ExternalBlock>>;

    /// Saves an executed transaction.
    async fn save_execution(&self, tx: TransactionExecution) -> anyhow::Result<()>;

    /// Reads all executed transactions.
    async fn read_executions(&self) -> anyhow::Result<Vec<TransactionExecution>>;

    /// Removes all executed transactions before the specified index.
    async fn remove_executions_before(&self, index: usize) -> anyhow::Result<()>;
}
