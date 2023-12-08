use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::Transaction;
use crate::eth::primitives::TransactionExecution;
use crate::eth::EthError;

/// EVM storage operations.
///
/// TODO: Evaluate if it should be split in multiple traits like EthAccountStorage, EthSlotStorage, EthTransactionStorage, etc.
pub trait EthStorage: Send + Sync + 'static {
    /// Retrieves an account from the storage.
    ///
    /// It should return empty empty account when not found.
    fn read_account(&self, address: &Address) -> Result<Account, EthError>;

    /// Retrieves an slot from the storage.
    ///
    /// It should return empty slot when not found.
    fn read_slot(&self, address: &Address, slot: &SlotIndex) -> Result<Slot, EthError>;

    /// Retrieves a transaction from the storage.
    ///
    /// It should return `None` when not found.
    fn read_transaction(&self, hash: &Hash) -> Result<Option<Transaction>, EthError>;

    /// Persist atomically all changes from a transaction execution.
    ///
    /// Before applying changes, it checks the storage current state matches the transaction execution previous state.
    fn save_execution(&self, transaction: Transaction, execution: TransactionExecution) -> Result<(), EthError>;
}
