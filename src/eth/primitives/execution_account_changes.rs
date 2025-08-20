use std::collections::BTreeMap;

use display_json::DebugAsJson;

use super::CodeHash;
use crate::alias::RevmBytecode;
use crate::eth::primitives::Account;
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
    pub code_hash: CodeHash,                                    // TODO: should be wrapped in a ExecutionValueChange (might be useless)
    pub slots: BTreeMap<SlotIndex, ExecutionValueChange<Slot>>, // TODO: should map idx to slotvalue
}

impl ExecutionAccountChanges {
    /// Creates a new [`ExecutionAccountChanges`] from Account original values.
    pub fn from_original_values(account: impl Into<Account>) -> Self {
        let account: Account = account.into();
        Self {
            nonce: ExecutionValueChange::from_original(account.nonce),
            balance: ExecutionValueChange::from_original(account.balance),
            bytecode: ExecutionValueChange::from_original(account.bytecode),
            slots: BTreeMap::new(),
            code_hash: account.code_hash,
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
            code_hash: account.code_hash,

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
}
