use std::collections::HashMap;

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
    pub bytecode: ExecutionValueChange<Option<Bytes>>,
    pub slots: HashMap<SlotIndex, ExecutionValueChange<Slot>>,
}

impl ExecutionAccountChanges {
    /// Creates a new [`ExecutionAccountChanges`] that represents an existing account.
    pub fn from_existing_account(account: impl Into<Account>) -> Self {
        let account = account.into();
        Self {
            new_account: false,
            address: account.address,
            nonce: ExecutionValueChange::from_original(account.nonce),
            balance: ExecutionValueChange::from_original(account.balance),
            bytecode: ExecutionValueChange::from_original(account.bytecode),
            slots: HashMap::new(),
        }
    }

    /// Creates a new [`ExecutionAccountChanges`] that represents an account being created by this transaction.
    pub fn from_new_account(account: Account, modified_slots: Vec<Slot>) -> Self {
        let mut changes = Self {
            new_account: true,
            address: account.address,
            nonce: ExecutionValueChange::from_modified(account.nonce),
            balance: ExecutionValueChange::from_modified(account.balance),
            bytecode: ExecutionValueChange::from_modified(account.bytecode),
            slots: HashMap::new(),
        };

        for slot in modified_slots {
            changes.slots.insert(slot.index.clone(), ExecutionValueChange::from_modified(slot));
        }

        changes
    }

    /// Updates an existing account state with changes that happened during the transaction.
    pub fn apply_changes(&mut self, account: Account, modified_slots: Vec<Slot>) {
        self.nonce.set_modified(account.nonce);
        self.balance.set_modified(account.balance);

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
