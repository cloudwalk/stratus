use std::collections::BTreeMap;

use display_json::DebugAsJson;

use crate::alias::RevmBytecode;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::ExecutionValueChange;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::Wei;

/// Changes that happened to an account during a transaction.
#[derive(DebugAsJson, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct ExecutionAccountChanges {
    pub nonce: ExecutionValueChange<Nonce>,
    pub balance: ExecutionValueChange<Wei>,

    // TODO: bytecode related information should be grouped in a Bytecode struct
    #[dummy(default)]
    pub bytecode: ExecutionValueChange<Option<RevmBytecode>>,
    pub slots: BTreeMap<SlotIndex, ExecutionValueChange<Slot>>, // TODO: should map idx to slotvalue
}

impl ExecutionAccountChanges {
    /// Merges another [`ExecutionAccountChanges`] into this one, replacing self values with values from other.
    /// For slots, performs a union of the BTrees, giving preference to values from other.
    pub fn merge(&mut self, other: ExecutionAccountChanges) {
        self.nonce = ExecutionValueChange {
            original: self.nonce.original,
            modified: if other.nonce.modified.is_set() {
                other.nonce.modified
            } else {
                self.nonce.modified
            },
        };
        self.balance = ExecutionValueChange {
            original: self.balance.original,
            modified: if other.balance.modified.is_set() {
                other.balance.modified
            } else {
                self.balance.modified
            },
        };
        self.bytecode = ExecutionValueChange {
            original: self.bytecode.original.clone(),
            modified: if other.bytecode.modified.is_set() {
                other.bytecode.modified
            } else {
                self.bytecode.modified.clone()
            },
        };

        // Merge slots, giving preference to values from other
        for (slot_index, slot_change) in other.slots {
            match self.slots.get_mut(&slot_index) {
                Some(existing_slot) => {
                    *existing_slot = ExecutionValueChange {
                        original: existing_slot.original.clone(),
                        modified: if slot_change.modified.is_set() {
                            slot_change.modified
                        } else {
                            existing_slot.modified.clone()
                        },
                    };
                }
                None => {
                    self.slots.insert(slot_index, slot_change);
                }
            }
        }
    }

    /// Creates a new [`ExecutionAccountChanges`] from Account original values.
    pub fn from_original_values(account: impl Into<Account>) -> Self {
        let account: Account = account.into();
        Self {
            nonce: ExecutionValueChange::from_original(account.nonce),
            balance: ExecutionValueChange::from_original(account.balance),
            bytecode: ExecutionValueChange::from_original(account.bytecode),
            slots: BTreeMap::new(),
        }
    }

    /// Creates a new [`ExecutionAccountChanges`] from Account modified values.
    pub fn from_modified_values(account: impl Into<Account>, modified_slots: Vec<Slot>) -> Self {
        let account: Account = account.into();
        let mut changes = Self {
            nonce: ExecutionValueChange::from_modified(account.nonce),
            balance: ExecutionValueChange::from_modified(account.balance),

            // bytecode
            bytecode: ExecutionValueChange::from_modified(account.bytecode),
            slots: BTreeMap::new(),
        };

        for slot in modified_slots {
            changes.slots.insert(slot.index, ExecutionValueChange::from_modified(slot));
        }

        changes
    }

    /// Updates an existing account state with changes that happened during the transaction.
    pub fn apply_modifications(&mut self, modified_account: Account, modified_slots: Vec<Slot>) {
        // update nonce if modified
        let is_nonce_modified = match self.nonce.take_original_ref() {
            Some(original_nonce) => *original_nonce != modified_account.nonce,
            None => true,
        };
        if is_nonce_modified {
            self.nonce.set_modified(modified_account.nonce);
        }

        // update balance if modified
        let is_balance_modified = match self.balance.take_original_ref() {
            Some(original_balance) => *original_balance != modified_account.balance,
            None => true,
        };
        if is_balance_modified {
            self.balance.set_modified(modified_account.balance);
        }

        // update all slots because all of them are modified
        for slot in modified_slots {
            match self.slots.get_mut(&slot.index) {
                Some(ref mut entry) => {
                    entry.set_modified(slot);
                }
                None => {
                    self.slots.insert(slot.index, ExecutionValueChange::from_modified(slot));
                }
            };
        }
    }

    /// Checks if account nonce, balance or bytecode were modified.
    pub fn is_account_modified(&self) -> bool {
        self.nonce.is_modified() || self.balance.is_modified() || self.bytecode.is_modified()
    }

    pub fn to_account(self, address: Address) -> Account {
        Account {
            address,
            nonce: self.nonce.take().unwrap_or_default(),
            balance: self.balance.take().unwrap_or_default(),
            bytecode: self.bytecode.take().unwrap_or_default(),
        }
    }
}
