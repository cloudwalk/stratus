use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::TransactionMined;
use crate::eth::EthError;

/// EVM storage operations.
///
/// TODO: Evaluate if it should be split in multiple traits like EthAccountStorage, EthSlotStorage, EthTransactionStorage, etc.
pub trait EthStorage: Send + Sync + 'static {
    // -------------------------------------------------------------------------
    // Block number operations
    // -------------------------------------------------------------------------

    // Retrieves the last mined block number.
    fn read_current_block_number(&self) -> Result<BlockNumber, EthError>;

    /// Translates a block selection to a specific block number.
    ///
    /// If the latest block is specified, `None` is returned because the block number is not known yet.
    fn translate_to_block_number(&self, block_selection: &BlockSelection) -> Result<Option<BlockNumber>, EthError>;

    /// Atomically increments the block number, returning the new value.
    fn increment_block_number(&self) -> Result<BlockNumber, EthError>;

    // -------------------------------------------------------------------------
    // State operations
    // -------------------------------------------------------------------------

    /// Retrieves an account from the storage.
    ///
    /// It should return empty empty account when not found.
    fn read_account(&self, address: &Address) -> Result<Account, EthError>;

    /// Retrieves an slot from the storage.
    ///
    /// It should return empty slot when not found.
    fn read_slot(&self, address: &Address, slot: &SlotIndex) -> Result<Slot, EthError>;

    /// Retrieves a block from the storage.
    ///
    /// It should return `None` when not found.
    fn read_block(&self, selection: &BlockSelection) -> Result<Option<Block>, EthError>;

    /// Retrieves a transaction from the storage.
    ///
    /// It should return `None` when not found.
    fn read_mined_transaction(&self, hash: &Hash) -> Result<Option<TransactionMined>, EthError>;

    /// Persist atomically all changes from a block.
    ///
    /// Before applying changes, it checks the storage current state matches the transaction previous state.
    fn save_block(&self, block: Block) -> Result<(), EthError>;
}
