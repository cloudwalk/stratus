use std::ops::Deref;

use display_json::DebugAsJson;

use crate::alias::RevmBytecode;
use crate::eth::types::Account;
use crate::eth::types::Address;
use crate::eth::types::Nonce;
use crate::eth::types::Wei;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(serde::Deserialize, fake::Dummy))]
pub enum ChangeValue<T>
where
    T: PartialEq + Eq + Default,
{
    Original(T),
    Changed(T),
}

impl<T: PartialEq + Eq + Default> Default for ChangeValue<T> {
    fn default() -> Self {
        Self::Original(T::default())
    }
}

impl<T: PartialEq + Eq + Default> Deref for ChangeValue<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Changed(inner) => inner,
            Self::Original(inner) => inner,
        }
    }
}

impl<T> ChangeValue<T>
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

    pub fn from_diff(original: T, current: T) -> Self {
        match original == current {
            true => Self::Original(current),
            false => Self::Changed(current),
        }
    }
}

impl<T, U> From<Option<U>> for ChangeValue<T>
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
#[derive(DebugAsJson, Clone, PartialEq, Eq, serde::Serialize, Default)]
#[cfg_attr(test, derive(serde::Deserialize, fake::Dummy))]
pub struct AccountChanges {
    pub nonce: ChangeValue<Nonce>,
    pub balance: ChangeValue<Wei>,
    #[cfg_attr(test, dummy(default))]
    pub bytecode: ChangeValue<Option<RevmBytecode>>,
}

impl AccountChanges {
    /// Updates an existing account state with changes that happened during the transaction.
    pub fn apply_modifications(&mut self, modified_account: Account) {
        self.nonce.apply(modified_account.nonce);
        self.balance.apply(modified_account.balance);
        self.bytecode.apply(modified_account.bytecode);
    }

    pub fn merge(&mut self, other: AccountChanges) {
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
}

impl From<(Address, AccountChanges)> for Account {
    fn from((address, change): (Address, AccountChanges)) -> Self {
        change.to_account(address)
    }
}

impl From<revm_state::Account> for AccountChanges {
    fn from(mut value: revm_state::Account) -> Self {
        let changed = std::mem::take(&mut value.info);
        let original = value.original_info_mut();
        Self {
            nonce: ChangeValue::from_diff(original.nonce.into(), changed.nonce.into()),
            balance: ChangeValue::from_diff(original.balance.into(), changed.balance.into()),
            bytecode: ChangeValue::from_diff(original.code.take(), changed.code),
        }
    }
}
