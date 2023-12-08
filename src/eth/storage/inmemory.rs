//! In-memory storage implementations

use std::collections::HashMap;
use std::sync::RwLock;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::Transaction;
use crate::eth::primitives::TransactionExecution;
use crate::eth::storage::EthStorage;
use crate::eth::EthError;

/// In-memory implementation using HashMaps.
#[derive(Debug, Default)]
pub struct InMemoryStorage {
    pub accounts: RwLock<HashMap<Address, Account>>,
    pub account_slots: RwLock<HashMap<Address, HashMap<SlotIndex, Slot>>>,
    pub transactions: RwLock<HashMap<Hash, (Transaction, TransactionExecution)>>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

impl EthStorage for InMemoryStorage {
    fn read_account(&self, address: &Address) -> Result<Account, EthError> {
        tracing::info!(%address, "reading account");

        let lock = self.accounts.read().unwrap();
        match lock.get(address).cloned() {
            Some(account) => {
                let bytecode_len = account.bytecode.as_ref().map(|x| x.len()).unwrap_or_default();
                tracing::trace!(%address, %bytecode_len, "account found");
                Ok(account)
            }
            None => {
                tracing::trace!(%address, "account not found");
                Ok(Account::default())
            }
        }
    }

    fn read_slot(&self, address: &Address, slot_index: &SlotIndex) -> Result<Slot, EthError> {
        tracing::info!(%address, %slot_index, "reading slot");

        let lock = self.account_slots.read().unwrap();
        let slots = match lock.get(address) {
            Some(slots) => slots,
            None => {
                tracing::trace!(%address, "account slots not found");
                return Ok(Slot::default());
            }
        };
        match slots.get(slot_index) {
            Some(slot) => {
                tracing::trace!(%address, %slot_index, %slot, "slot found");
                Ok(slot.clone())
            }
            None => {
                tracing::trace!(%address, %slot_index, "account found, but slot not found");
                Ok(Slot::default())
            }
        }
    }

    fn read_transaction(&self, hash: &Hash) -> Result<Option<Transaction>, EthError> {
        tracing::info!(%hash, "reading transaction");
        let lock = self.transactions.read().unwrap();
        match lock.get(hash) {
            Some(transaction) => {
                tracing::trace!(%hash, ?transaction, "transaction found");
                Ok(Some(transaction.0.clone()))
            }
            None => {
                tracing::trace!(%hash, "transaction not found");
                Ok(None)
            }
        }
    }

    fn save_execution(&self, transaction: Transaction, execution: TransactionExecution) -> Result<(), EthError> {
        let mut account_lock = self.accounts.write().unwrap();
        let mut account_slots_lock = self.account_slots.write().unwrap();
        let mut transactions_lock = self.transactions.write().unwrap();

        // save transaction
        tracing::info!(hash = %transaction.hash(), "saving transaction");
        transactions_lock.insert(transaction.hash(), (transaction, execution.clone()));

        // save execution changes
        let execution_changes = execution.changes.clone();
        for (address, changes) in execution_changes {
            tracing::debug!(%address, "saving changes");
            let account = account_lock.entry(address.clone()).or_default();
            let account_slots = account_slots_lock.entry(address).or_default();

            // nonce
            tracing::trace!(nonce = %changes.nonce, "saving nonce");
            account.nonce = changes.nonce.clone();

            // bytecode
            if let Some(bytecode) = changes.bytecode.clone() {
                tracing::trace!(bytecode_len = %bytecode.len(), "saving bytecode");
                account.bytecode = Some(bytecode);
            }

            // storage
            for slot in changes.slots.clone() {
                tracing::trace!(%slot, "saving slot");
                account_slots.insert(slot.index.clone(), slot);
            }
        }
        Ok(())
    }
}
