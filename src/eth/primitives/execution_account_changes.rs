use derive_more::Deref;
use display_json::DebugAsJson;

use crate::alias::RevmBytecode;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Wei;

#[derive(Debug, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize, Default, Deref)]
pub struct Change<T>
where
    T: PartialEq + Eq + Default,
{
    #[deref]
    pub value: T,
    pub changed: bool,
}

impl<T> Change<T>
where
    T: PartialEq + Eq + Default,
{
    pub fn apply(&mut self, value: T) {
        if self.value != value {
            self.value = value;
            self.changed = true;
        }
    }
}

/// Changes that happened to an account during a transaction.
#[derive(DebugAsJson, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize, Default)]
pub struct ExecutionAccountChanges {
    pub nonce: Change<Nonce>,
    pub balance: Change<Wei>,
    #[dummy(default)]
    pub bytecode: Change<Option<RevmBytecode>>,
}

impl ExecutionAccountChanges {
    /// Updates an existing account state with changes that happened during the transaction.
    pub fn apply_modifications(&mut self, modified_account: Account) {
        self.nonce.apply(modified_account.nonce);
        self.balance.apply(modified_account.balance);
        self.bytecode.apply(modified_account.bytecode);
    }

    /// Checks if account nonce, balance or bytecode were modified.
    pub fn is_modified(&self) -> bool {
        self.nonce.changed || self.balance.changed || self.bytecode.changed
    }

    pub fn to_account(self, address: Address) -> Account {
        Account {
            address,
            nonce: self.nonce.value,
            balance: self.balance.value,
            bytecode: self.bytecode.value,
        }
    }

    pub fn from_changed(account: Account) -> Self {
        Self {
            nonce: Change {
                value: account.nonce,
                changed: true,
            },
            balance: Change {
                value: account.balance,
                changed: true,
            },
            bytecode: Change {
                value: account.bytecode,
                changed: true,
            },
        }
    }

    pub fn from_unchanged(account: Account) -> Self {
        Self {
            nonce: Change {
                value: account.nonce,
                changed: false,
            },
            balance: Change {
                value: account.balance,
                changed: false,
            },
            bytecode: Change {
                value: account.bytecode,
                changed: false,
            },
        }
    }
}

impl From<(Address, ExecutionAccountChanges)> for Account {
    fn from((address, change): (Address, ExecutionAccountChanges)) -> Self {
        Self { address, nonce: change.nonce.value, balance: change.balance.value, bytecode: change.bytecode.value }
    }
}
