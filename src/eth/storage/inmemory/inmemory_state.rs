//! In-memory storage implementations.

use std::collections::HashMap;

use super::inmemory_account::InMemoryAccount;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Execution;
use crate::eth::primitives::ExecutionConflicts;
use crate::eth::primitives::ExecutionConflictsBuilder;

/// The inmemory storage is split into two structs. InMemoryStoragePermanent and
/// InMemoryStorageTemporary to facilitate debugging when using the inmemory storage
/// for both temp and perm contexts.

/// In-memory implementation using maps.


pub trait StorageState<T: InMemoryAccount> {
    fn get_accounts(&self) -> &HashMap<Address, T>;
    fn get_accounts_mut(&mut self) -> &mut HashMap<Address, T>;

    fn save_account_changes(&mut self, block_number: BlockNumber, execution: Execution) {
        let is_success = execution.is_success();
        for changes in execution.changes {
            let account = self
                .get_accounts_mut()
                .entry(changes.address.clone())
                .or_insert_with(|| T::new(changes.address));

            // account basic info
            if let Some(nonce) = changes.nonce.take() {
                account.set_nonce(block_number, nonce);
            }
            if let Some(balance) = changes.balance.take() {
                account.set_balance(block_number, balance);
            }
            if let Some(Some(bytecode)) = changes.bytecode.take() {
                account.set_bytecode(block_number, bytecode);
            }

            // slots
            if is_success {
                for (_, slot) in changes.slots {
                    if let Some(slot) = slot.take_modified() {
                        account.set_slot(block_number, slot);
                    }
                }
            }
        }
    }

    fn check_conflicts(&self, execution: &Execution) -> Option<ExecutionConflicts> {
        let mut conflicts = ExecutionConflictsBuilder::default();

        for change in &execution.changes {
            let address = &change.address;

            if let Some(account) = self.get_accounts().get(address) {
                // check account info conflicts
                if let Some(touched_nonce) = change.nonce.take_original_ref() {
                    let nonce = account.get_current_nonce();
                    if touched_nonce != nonce {
                        conflicts.add_nonce(address.clone(), nonce.clone(), touched_nonce.clone());
                    }
                }
                if let Some(touched_balance) = change.balance.take_original_ref() {
                    let balance = account.get_current_balance();
                    if touched_balance != balance {
                        conflicts.add_balance(address.clone(), balance.clone(), touched_balance.clone());
                    }
                }

                // check slots conflicts
                for (touched_slot_index, touched_slot) in &change.slots {
                    if let Some(slot) = account.get_current_slot(touched_slot_index) {
                        if let Some(touched_slot) = touched_slot.take_original_ref() {
                            let slot_value = slot.value.clone();
                            if touched_slot.value != slot_value {
                                conflicts.add_slot(address.clone(), touched_slot_index.clone(), slot_value, touched_slot.value.clone());
                            }
                        }
                    }
                }
            }
        }
        conflicts.build()
    }
}
