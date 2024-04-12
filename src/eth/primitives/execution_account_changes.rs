use std::collections::HashMap;

use super::CodeHash;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ExecutionValueChange;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::Wei;
use crate::ext::not;

/// Changes that happened to an account during a transaction.
#[derive(Debug, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct ExecutionAccountChanges {
    new_account: bool,
    pub address: Address,
    pub nonce: ExecutionValueChange<Nonce>,
    pub balance: ExecutionValueChange<Wei>,

    // TODO: bytecode related information should be grouped in a Bytecode struct
    pub bytecode: ExecutionValueChange<Option<Bytes>>,
    pub code_hash: CodeHash, // TODO: should be wrapped in a ExecutionValueChange

    pub slots: HashMap<SlotIndex, ExecutionValueChange<Slot>>,
}

impl ExecutionAccountChanges {
    /// Creates a new [`ExecutionAccountChanges`] from Account original values.
    pub fn from_original_values(account: impl Into<Account>) -> Self {
        let account: Account = account.into();
        Self {
            new_account: false,
            address: account.address,
            nonce: ExecutionValueChange::from_original(account.nonce),
            balance: ExecutionValueChange::from_original(account.balance),
            bytecode: ExecutionValueChange::from_original(account.bytecode),
            slots: HashMap::new(),
            code_hash: account.code_hash,
        }
    }

    /// Creates a new [`ExecutionAccountChanges`] from Account modified values.
    pub fn from_modified_values(account: impl Into<Account>, modified_slots: Vec<Slot>) -> Self {
        let account: Account = account.into();
        let mut changes = Self {
            new_account: true,
            address: account.address,
            nonce: ExecutionValueChange::from_modified(account.nonce),
            balance: ExecutionValueChange::from_modified(account.balance),

            // bytecode
            bytecode: ExecutionValueChange::from_modified(account.bytecode),
            code_hash: account.code_hash,

            slots: HashMap::new(),
        };

        for slot in modified_slots {
            changes.slots.insert(slot.index.clone(), ExecutionValueChange::from_modified(slot));
        }

        changes
    }

    /// Updates an existing account state with changes that happened during the transaction.
    pub fn apply_modifications(&mut self, modified_account: Account, modified_slots: Vec<Slot>) {
        // update nonce if modified
        let nonce_modified = match self.nonce.take_original_ref() {
            Some(nonce) => *nonce != modified_account.nonce,
            None => true,
        };
        if nonce_modified {
            self.nonce.set_modified(modified_account.nonce);
        }

        // update balance if modified
        let balance_modified = match self.balance.take_modified_ref() {
            Some(balance) => *balance != modified_account.balance,
            None => true,
        };
        if balance_modified {
            self.balance.set_modified(modified_account.balance);
        }

        // update all slots because all of them are modified
        for slot in modified_slots {
            match self.slots.get_mut(&slot.index) {
                Some(ref mut entry) => {
                    entry.set_modified(slot);
                }
                None => {
                    self.slots.insert(slot.index.clone(), ExecutionValueChange::from_modified(slot));
                }
            };
        }
    }

    /// Checks if the account was created by this transaction.
    pub fn is_account_creation(&self) -> bool {
        self.new_account
    }

    /// Checks if the account was updated by this transaction (it must already exist).
    pub fn is_account_update(&self) -> bool {
        not(self.new_account)
    }
}
