use super::entities::Account;
use super::entities::Address;
use super::entities::Slot;
use super::entities::SlotIndex;
use super::entities::TransactionExecution;
use super::EvmError;

/// EVM storage operations.
pub trait EvmStorage {
    /// Retrieves an account from the storage.
    ///
    /// When not found, returns an empty account because EVM assumes all account exists.
    fn get_account(&self, address: &Address) -> Result<Account, EvmError>;

    /// Retrieves an slot from the storage.
    ///
    /// When not found, return an empty slot because EVM assumes all slots exists with zero value.
    fn get_slot(&self, address: &Address, slot: &SlotIndex) -> Result<Slot, EvmError>;

    /// Persist atomically all changes from a transaction execution.
    ///
    /// Before applying changes, it checks the storage current state matches the transaction execution previous state.
    fn save_execution(&self, execution: &TransactionExecution) -> Result<(), EvmError>;
}
