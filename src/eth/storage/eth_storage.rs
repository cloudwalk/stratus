use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::TransactionExecution;
use crate::eth::EthError;

/// EVM storage operations.
///
/// TODO: Evaluate if it should be split in multiple traits like EthAccountStorage, EthSlotStorage, EthTransactionStorage, etc.
pub trait EthStorage: Send + Sync + 'static {
    /// Retrieves an account from the storage.
    ///
    /// When not found, returns an empty account because EVM assumes all account exists.
    fn read_account(&self, address: &Address) -> Result<Account, EthError>;

    /// Retrieves an slot from the storage.
    ///
    /// When not found, return an empty slot because EVM assumes all slots exists with zero value.
    fn read_slot(&self, address: &Address, slot: &SlotIndex) -> Result<Slot, EthError>;

    /// Persist atomically all changes from a transaction execution.
    ///
    /// Before applying changes, it checks the storage current state matches the transaction execution previous state.
    fn save_execution(&self, execution: TransactionExecution) -> Result<(), EthError>;
}
