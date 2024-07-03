use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::ExecutionConflicts;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::Hash;
use crate::eth::primitives::PendingBlock;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::TransactionExecution;

/// Temporary storage (in-between blocks) operations
pub trait TemporaryStorage: Send + Sync + 'static {
    // -------------------------------------------------------------------------
    // Block number
    // -------------------------------------------------------------------------

    /// Sets the block number being mined.
    fn set_pending_block_number(&self, number: BlockNumber) -> anyhow::Result<()>;

    // Retrieves the block number being mined.
    fn read_pending_block_number(&self) -> anyhow::Result<Option<BlockNumber>>;

    // -------------------------------------------------------------------------
    // Block and executions
    // -------------------------------------------------------------------------

    /// Sets the pending external block being re-executed.
    fn set_pending_external_block(&self, block: ExternalBlock) -> anyhow::Result<()>;

    /// Saves a re-executed transaction to the pending mined block.
    fn save_execution(&self, tx: TransactionExecution) -> anyhow::Result<()>;

    /// Retrieves the pending transactions of the pending block.
    fn pending_transactions(&self) -> anyhow::Result<Vec<TransactionExecution>>;

    /// Finishes the mining of the pending block and starts a new block.
    fn finish_pending_block(&self) -> anyhow::Result<PendingBlock>;

    /// Retrieves a transaction from the storage.
    fn read_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionExecution>>;

    // -------------------------------------------------------------------------
    // Accounts and slots
    // -------------------------------------------------------------------------

    /// Checks if an execution conflicts with current storage state.
    fn check_conflicts(&self, execution: &EvmExecution) -> anyhow::Result<Option<ExecutionConflicts>>;

    /// Retrieves an account from the storage. Returns Option when not found.
    fn read_account(&self, address: &Address) -> anyhow::Result<Option<Account>>;

    /// Retrieves an slot from the storage. Returns Option when not found.
    fn read_slot(&self, address: &Address, index: &SlotIndex) -> anyhow::Result<Option<Slot>>;

    // -------------------------------------------------------------------------
    // Global state
    // -------------------------------------------------------------------------

    /// Resets to default empty state.
    fn reset(&self) -> anyhow::Result<()>;
}
