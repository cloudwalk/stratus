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
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::MetrifiedStorage;
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

    // -------------------------------------------------------------------------
    // State operations
    // -------------------------------------------------------------------------

    /// Retrieves an account from the storage.
    ///
    /// It should return empty empty account when not found.
    fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> Result<Account, EthError>;

    /// Retrieves an slot from the storage.
    ///
    /// It should return empty slot when not found.
    fn read_slot(&self, address: &Address, slot: &SlotIndex, point_in_time: &StoragePointInTime) -> Result<Slot, EthError>;

    /// Retrieves a block from the storage.
    ///
    /// It should return `None` when not found.
    fn read_block(&self, block_selection: &BlockSelection) -> Result<Option<Block>, EthError>;

    /// Retrieves a transaction from the storage.
    ///
    /// It should return `None` when not found.
    fn read_mined_transaction(&self, hash: &Hash) -> Result<Option<TransactionMined>, EthError>;

    /// Retrieves logs from the storage.
    fn read_logs(&self, filter: &LogFilter) -> Result<Vec<LogMined>, EthError>;

    /// Persist atomically all changes from a block.
    ///
    /// Before applying changes, it checks the storage current state matches the transaction previous state.
    fn save_block(&self, block: Block) -> Result<(), EthError>;

    // -------------------------------------------------------------------------
    // Default operations
    // -------------------------------------------------------------------------

    /// Wraps the current storage with a proxy that collects execution metrics.
    fn metrified(self) -> MetrifiedStorage<Self>
    where
        Self: Sized,
    {
        MetrifiedStorage::new(self)
    }

    /// Translates a block selection to a specific storage point-in-time indicator.
    fn translate_to_point_in_time(&self, block_selection: &BlockSelection) -> Result<StoragePointInTime, EthError> {
        match block_selection {
            BlockSelection::Latest => Ok(StoragePointInTime::Present),
            BlockSelection::Number(number) => {
                let current_block = self.read_current_block_number()?;
                if number <= &current_block {
                    Ok(StoragePointInTime::Past(*number))
                } else {
                    Err(EthError::InvalidBlockSelection)
                }
            }
            BlockSelection::Earliest | BlockSelection::Hash(_) => match self.read_block(block_selection)? {
                Some(block) => Ok(StoragePointInTime::Past(block.header.number)),
                None => Err(EthError::InvalidBlockSelection),
            },
        }
    }
}

/// Retrieves test accounts.
///
/// TODO: use a feature-flag to determine if test accounts should be returned.
pub fn test_accounts() -> Vec<Account> {
    use hex_literal::hex;

    use crate::eth::primitives::Wei;

    [
        hex!("f39fd6e51aad88f6f4ce6ab8827279cfffb92266"),
        hex!("70997970c51812dc3a010c7d01b50e0d17dc79c8"),
        hex!("3c44cdddb6a900fa2b585dd299e03d12fa4293bc"),
        hex!("15d34aaf54267db7d7c367839aaf71a00a2c6a65"),
    ]
    .into_iter()
    .map(|address| Account {
        address: address.into(),
        balance: Wei::MAX,
        ..Account::default()
    })
    .collect()
}
