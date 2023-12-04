//! In-memory storage implementations.

use std::collections::HashMap;
use std::sync::RwLock;

use crate::evm::entities::Account;
use crate::evm::entities::Address;
use crate::evm::entities::Slot;
use crate::evm::entities::SlotIndex;
use crate::evm::entities::TransactionExecution;
use crate::evm::EvmError;
use crate::evm::EvmStorage;

/// In-memory implementation using HashMaps.
#[derive(Debug, Default)]
pub struct InMemoryStorage {
    pub accounts: RwLock<HashMap<Address, Account>>,
    pub account_slots: RwLock<HashMap<Address, HashMap<SlotIndex, Slot>>>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

impl EvmStorage for InMemoryStorage {
    fn get_account(&self, address: &Address) -> Result<Account, EvmError> {
        tracing::debug!(%address, "retrieving account");

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

    fn get_slot(&self, address: &Address, slot_index: &SlotIndex) -> Result<Slot, EvmError> {
        tracing::debug!(%address, %slot_index, "retrieving slot");

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

    fn save_execution(&self, execution: &TransactionExecution) -> Result<(), EvmError> {
        let mut account_lock = self.accounts.write().unwrap();
        let mut account_slots_lock = self.account_slots.write().unwrap();

        let execution_changes = execution.changes.clone();
        for (address, changes) in execution_changes {
            tracing::debug!(%address, "saving changes");
            let account = account_lock.entry(address.clone()).or_default();
            let account_slots = account_slots_lock.entry(address.clone()).or_default();

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
