use async_trait::async_trait;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::ExecutionConflicts;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::TransactionExecution;

/// Temporary storage (in-between blocks) operations
#[async_trait]
pub trait TemporaryStorage: Send + Sync {
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

    // Reads the block number activelly being mined.
    async fn read_active_block_number(&self) -> anyhow::Result<Option<BlockNumber>>;

    // -------------------------------------------------------------------------
    // External block
    // -------------------------------------------------------------------------

    /// Sets the external block being re-executed.
    async fn set_external_block(&self, block: ExternalBlock) -> anyhow::Result<()>;

    /// Reads the external block of the last finished block.
    async fn read_pending_external_block(&self) -> anyhow::Result<Option<ExternalBlock>>;

    // -------------------------------------------------------------------------
    // Executions
    // -------------------------------------------------------------------------

    /// Saves an re-executed transaction to the active mined block.
    async fn save_execution(&self, tx: TransactionExecution) -> anyhow::Result<()>;

    /// Reads all executed transactions of the last finished block.
    async fn read_pending_executions(&self) -> anyhow::Result<Vec<TransactionExecution>>;

    // -------------------------------------------------------------------------
    // General state
    // -------------------------------------------------------------------------

    /// Checks if an execution conflicts with current storage state.
    async fn check_conflicts(&self, execution: &EvmExecution) -> anyhow::Result<Option<ExecutionConflicts>>;

    /// Finishes the mining of the active block and starts a new block.
    async fn finish_block(&self) -> anyhow::Result<BlockNumber>;

    /// Resets to default empty state.
    async fn reset(&self) -> anyhow::Result<()>;

    /// If necessary, flushes temporary state to durable storage.
    async fn flush(&self) -> anyhow::Result<()>;
}
