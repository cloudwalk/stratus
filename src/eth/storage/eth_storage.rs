use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragerPointInTime;
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

    /// Atomically increments the block number, returning the new value.
    fn increment_block_number(&self) -> Result<BlockNumber, EthError>;

    /// Translates a block selection to a specific storage point-in-time indicator.
    fn translate_to_point_in_time(&self, block_selection: &BlockSelection) -> Result<StoragerPointInTime, EthError> {
        match block_selection {
            BlockSelection::Latest => Ok(StoragerPointInTime::Present),
            BlockSelection::Number(number) => {
                let current_block = self.read_current_block_number()?;
                if number <= &current_block {
                    Ok(StoragerPointInTime::Past(number.clone()))
                } else {
                    Err(EthError::InvalidBlockSelection)
                }
            }
            BlockSelection::Hash(_) => match self.read_block(block_selection)? {
                Some(block) => Ok(StoragerPointInTime::Past(block.header.number.clone())),
                None => Err(EthError::InvalidBlockSelection),
            },
        }
    }

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
    fn read_slot(&self, address: &Address, slot: &SlotIndex, point_in_time: &StoragerPointInTime) -> Result<Slot, EthError>;

    /// Retrieves a block from the storage.
    ///
    /// It should return `None` when not found.
    fn read_block(&self, block_selection: &BlockSelection) -> Result<Option<Block>, EthError>;

    /// Retrieves a transaction from the storage.
    ///
    /// It should return `None` when not found.
    fn read_mined_transaction(&self, hash: &Hash) -> Result<Option<TransactionMined>, EthError>;

    /// Persist atomically all changes from a block.
    ///
    /// Before applying changes, it checks the storage current state matches the transaction previous state.
    fn save_block(&self, block: Block) -> Result<(), EthError>;
}
