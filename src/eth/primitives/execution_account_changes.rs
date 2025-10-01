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
    value: T,
    changed: bool,
}

impl<T> Change<T>
where
    T: PartialEq + Eq + Default,
{
    /// Updates the value and marks it as changed if the new value differs from the current one.
    ///
    /// This method will only update the internal value and set the `changed` flag to `true`
    /// if the provided value is different from the current value.
    pub fn apply(&mut self, changed_value: T) {
        if self.value != changed_value {
            self.value = changed_value;
            self.changed = true;
        }
    }

    /// Sets the original value only if no changes have been applied yet.
    ///
    /// This method will update the internal value only if the `changed` flag is `false`,
    /// preserving any modifications that may have been made.
    fn apply_original(&mut self, value: T) {
        if !self.changed {
            self.value = value;
        }
    }

    /// Returns whether the value has been changed.
    pub fn is_changed(&self) -> bool {
        self.changed
    }

    /// Returns a reference to the current value.
    pub fn value(&self) -> &T {
        &self.value
    }
}

impl<T, U> From<Option<U>> for Change<T>
where
    T: PartialEq + Eq + Default,
    U: Into<T>,
{
    fn from(value: Option<U>) -> Self {
        match value {
            Some(value) => Self {
                value: value.into(),
                changed: true,
            },
            None => Self {
                changed: false,
                ..Default::default()
            },
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

    pub fn apply_original(&mut self, original_account: Account) {
        self.nonce.apply_original(original_account.nonce);
        self.balance.apply_original(original_account.balance);
        self.bytecode.apply_original(original_account.bytecode);
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
        Self {
            address,
            nonce: change.nonce.value,
            balance: change.balance.value,
            bytecode: change.bytecode.value,
        }
    }
}
