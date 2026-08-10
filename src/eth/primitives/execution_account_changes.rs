use std::ops::Deref;

use display_json::DebugAsJson;

use crate::alias::RevmBytecode;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Wei;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(fake::Dummy))]
pub enum AccountChangeValue<T>
where
    T: PartialEq + Eq + Default,
{
    Original(T),
    Changed(T),
}

impl<T: PartialEq + Eq + Default> Default for AccountChangeValue<T> {
    fn default() -> Self {
        Self::Original(T::default())
    }
}

impl<T: PartialEq + Eq + Default> Deref for AccountChangeValue<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Changed(inner) => inner,
            Self::Original(inner) => inner,
        }
    }
}

impl<T> AccountChangeValue<T>
where
    T: PartialEq + Eq + Default,
{
    pub fn value(&self) -> &T {
        match self {
            Self::Changed(value) => value,
            Self::Original(value) => value,
        }
    }

    pub fn take_value(self) -> T {
        match self {
            Self::Changed(value) => value,
            Self::Original(value) => value,
        }
    }

    /// Updates the value and marks it as changed if the new value differs from the current one.
    ///
    /// This method will only update the internal value and set the `changed` flag to `true`
    /// if the provided value is different from the current value.
    pub fn apply(&mut self, changed_value: T) {
        if self.value() != &changed_value {
            *self = Self::Changed(changed_value);
        }
    }

    /// Sets the original value only if no changes have been applied yet.
    ///
    /// This method will update the internal value only if the `changed` flag is `false`,
    /// preserving any modifications that may have been made.
    fn apply_original(&mut self, value: T) {
        if let Self::Original(original_value) = self {
            *original_value = value;
        }
    }

    /// Returns whether the value has been changed.
    pub fn is_changed(&self) -> bool {
        matches!(self, Self::Changed(_))
    }
}

impl<T, U> From<Option<U>> for AccountChangeValue<T>
where
    T: PartialEq + Eq + Default,
    U: Into<T>,
{
    fn from(value: Option<U>) -> Self {
        match value {
            Some(value) => Self::Changed(value.into()),
            None => Self::Original(T::default()),
        }
    }
}

/// Changes that happened to an account during a transaction.
#[derive(DebugAsJson, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct ExecutionAccountChanges {
    pub nonce: AccountChangeValue<Nonce>,
    pub balance: AccountChangeValue<Wei>,
    #[cfg_attr(test, dummy(default))]
    pub bytecode: AccountChangeValue<Option<RevmBytecode>>,
}

impl ExecutionAccountChanges {
    /// Updates an existing account state with changes that happened during the transaction.
    pub fn apply_modifications(&mut self, modified_account: Account) {
        self.nonce.apply(modified_account.nonce);
        self.balance.apply(modified_account.balance);
        self.bytecode.apply(modified_account.bytecode);
    }

    pub fn merge(&mut self, other: ExecutionAccountChanges) {
        if other.nonce.is_changed() {
            self.nonce = other.nonce;
        }
        if other.balance.is_changed() {
            self.balance = other.balance;
        }
        if other.bytecode.is_changed() {
            self.bytecode = other.bytecode;
        }
    }

    pub(crate) fn apply_original(&mut self, original_account: Account) {
        self.nonce.apply_original(original_account.nonce);
        self.balance.apply_original(original_account.balance);
        self.bytecode.apply_original(original_account.bytecode);
    }

    /// Checks if account nonce, balance or bytecode were modified.
    pub fn is_modified(&self) -> bool {
        self.nonce.is_changed() || self.balance.is_changed() || self.bytecode.is_changed()
    }

    pub fn to_account(self, address: Address) -> Account {
        Account {
            address,
            nonce: self.nonce.take_value(),
            balance: self.balance.take_value(),
            bytecode: self.bytecode.take_value(),
        }
    }

    pub fn from_changed(account: Account) -> Self {
        Self {
            nonce: AccountChangeValue::Changed(account.nonce),
            balance: AccountChangeValue::Changed(account.balance),
            bytecode: AccountChangeValue::Changed(account.bytecode),
        }
    }

    pub fn from_unchanged(account: Account) -> Self {
        Self {
            nonce: AccountChangeValue::Original(account.nonce),
            balance: AccountChangeValue::Original(account.balance),
            bytecode: AccountChangeValue::Original(account.bytecode),
        }
    }
}

impl From<(Address, ExecutionAccountChanges)> for Account {
    fn from((address, change): (Address, ExecutionAccountChanges)) -> Self {
        change.to_account(address)
    }
}
