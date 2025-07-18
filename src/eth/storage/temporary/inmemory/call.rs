//! In-memory call temporary storage implementation.

use std::collections::HashMap;

use dashmap::DashMap;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::TransactionExecution;
use crate::eth::storage::temporary::inmemory::TxCount;

#[derive(Debug)]
pub struct InMemoryCallTemporaryStorage {
    /// Storage for call temporary data indexed by (block_number, tx_count).
    /// tx_count is None for block-level data, Some(count) for transaction-level data.
    storage: DashMap<BlockNumber, InMemoryCallTemporaryStorageState>,
}

impl InMemoryCallTemporaryStorage {
    /// Creates a new instance of InmemoryCallTemporaryStorage.
    pub fn new() -> Self {
        Self {
            storage: DashMap::with_capacity(10),
        }
    }

    /// Reads the latest account data for the given address at the specified block and transaction.
    ///
    /// Returns the account data if found, otherwise None.
    pub fn read_account(&self, block: BlockNumber, tx: TxCount, address: Address) -> Option<Account> {
        if let Some(block_state) = self.storage.get(&block)
            && let Some(accounts) = block_state.accounts.get(&address)
        {
            return accounts.iter().rev().find(|(_, t)| *t <= tx).map(|(acc, _)| acc.clone());
        }
        None
    }

    /// Reads the latest slot value for the given address and slot index at the specified block and transaction.
    ///
    /// Returns the slot value if found, otherwise None.
    pub fn read_slot(&self, block: BlockNumber, tx: TxCount, address: Address, slot: SlotIndex) -> Option<Slot> {
        if let Some(block_state) = self.storage.get(&block)
            && let Some(slot_values) = block_state.slots.get(&(address, slot))
        {
            return slot_values
                .iter()
                .rev()
                .find(|(_, t)| *t <= tx)
                .map(|(value, _)| Slot { index: slot, value: *value });
        }
        None
    }

    /// Updates the storage by appending new entries from the given TransactionExecution.
    ///
    /// This function takes a transaction execution and appends all account and slot changes
    /// to the corresponding BlockNumber in the call storage with the given transaction count.
    pub fn update_state_with_transaction(&self, tx: &TransactionExecution) {
        let block_number = tx.evm_input.block_number;

        // Get or create the block state
        let mut block_state = self.storage.entry(block_number).or_default();
        block_state.current_tx_count += 1;
        let current_tx_count = block_state.current_tx_count;

        // Process each account change from the transaction execution
        for (address, change) in &tx.result.execution.changes {
            // Check if any account info changed
            let updated_nonce = change.nonce.is_modified();
            let updated_balance = change.balance.is_modified();
            let updated_bytecode = change.bytecode.is_modified();

            if updated_nonce || updated_balance || updated_bytecode {
                // Build the account from the changes
                let mut account = Account {
                    address: *address,
                    ..Default::default()
                };

                if let Some(nonce) = change.nonce.take_ref() {
                    account.nonce = *nonce;
                }

                if let Some(balance) = change.balance.take_ref() {
                    account.balance = *balance;
                }

                if let Some(Some(bytecode)) = change.bytecode.take_ref() {
                    account.bytecode = Some(bytecode.clone());
                }

                block_state.accounts.entry(*address).or_default().push((account, current_tx_count));
            }

            // Add slot changes
            for (slot_index, slot_change) in &change.slots {
                if let Some(slot) = slot_change.take_modified_ref() {
                    block_state
                        .slots
                        .entry((*address, *slot_index))
                        .or_default()
                        .push((slot.value, current_tx_count));
                }
            }
        }
    }

    /// Retains only the 10 most recent blocks and removes all older entries.
    pub fn retain_recent_blocks(&self) {
        let mut block_numbers: Vec<BlockNumber> = self.storage.iter().map(|r| *r.key()).collect();
        block_numbers.sort_by(|a, b| b.cmp(a)); // Sort in descending order

        // If we have more than 10 blocks, determine the cutoff point
        if block_numbers.len() > 9 {
            let cutoff = block_numbers[9];
            // Remove all blocks older than or equal to the cutoff
            self.storage.retain(|block_number, _| *block_number > cutoff);
        }
    }

    /// Clears all data from the call temporary storage.
    ///
    /// This function removes all stored block state data, effectively resetting the storage.
    pub fn reset(&self) {
        self.storage.clear();
    }
}

impl Default for InMemoryCallTemporaryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Default)]
pub struct InMemoryCallTemporaryStorageState {
    pub current_tx_count: TxCount,
    pub accounts: HashMap<Address, Vec<(Account, TxCount)>>,
    pub slots: HashMap<(Address, SlotIndex), Vec<(SlotValue, TxCount)>>,
}
