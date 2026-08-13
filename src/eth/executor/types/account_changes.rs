use std::ops::Deref;

use crate::alias::RevmBytecode;
use crate::eth::executor::types::execution_changes::Complete;
use crate::eth::executor::types::execution_changes::Incomplete;
use crate::eth::executor::types::execution_changes::Stage;
use crate::eth::types::Account;
use crate::eth::types::Address;
use crate::eth::types::Nonce;
use crate::eth::types::Wei;

/// Complete-stage field: a real value, either the original (untouched by the block) or changed by it.
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

/// Incomplete-stage field: either the block changed this value, or the original is not yet known
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum Unset<T> {
    Changed(T),
    #[default]
    Unset,
}

impl<T> Unset<T> {
    /// Resolves an incomplete field into a complete one, using `original` when the block did not touch it.
    pub fn complete(self, original: T) -> ChangeValue<T>
    where
        T: PartialEq + Eq + Default,
    {
        match self {
            Self::Changed(value) => ChangeValue::Changed(value),
            Self::Unset => ChangeValue::Original(original),
        }
    }
}

impl<T, U> From<Option<U>> for Unset<T>
where
    U: Into<T>,
{
    fn from(value: Option<U>) -> Self {
        match value {
            Some(value) => Self::Changed(value.into()),
            None => Self::Unset,
        }
    }
}

/// Changes that happened to an account during a transaction.
#[derive(Clone, PartialEq, Eq, serde::Serialize, Default)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct AccountChanges<S: Stage = Complete> {
    pub nonce: S::Field<Nonce>,
    pub balance: S::Field<Wei>,
    pub bytecode: S::Field<Option<RevmBytecode>>,
}

impl AccountChanges<Incomplete> {
    /// Fills every unset field with its real original value, advancing to [`Complete`].
    pub fn complete(self, original: Account) -> AccountChanges<Complete> {
        AccountChanges {
            nonce: self.nonce.complete(original.nonce),
            balance: self.balance.complete(original.balance),
            bytecode: self.bytecode.complete(original.bytecode),
        }
    }
}

impl AccountChanges<Complete> {
    /// Updates an existing account state with changes that happened during the transaction.
    pub fn apply_modifications(&mut self, modified_account: Account) {
        self.nonce.apply(modified_account.nonce);
        self.balance.apply(modified_account.balance);
        self.bytecode.apply(modified_account.bytecode);
    }

    pub fn merge(&mut self, other: AccountChanges<Complete>) {
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

impl From<(Address, AccountChanges<Complete>)> for Account {
    fn from((address, change): (Address, AccountChanges<Complete>)) -> Self {
        change.to_account(address)
    }
}

impl From<revm_state::Account> for AccountChanges<Complete> {
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

impl<S: Stage> std::fmt::Debug for AccountChanges<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&serde_json::to_string(self).expect("AccountChanges must be serializable for Debug"))
    }
}

#[cfg(test)]
impl fake::Dummy<fake::Faker> for AccountChanges<Complete> {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(faker: &fake::Faker, rng: &mut R) -> Self {
        Self {
            nonce: fake::Dummy::dummy_with_rng(faker, rng),
            balance: fake::Dummy::dummy_with_rng(faker, rng),
            bytecode: ChangeValue::default(),
        }
    }
}
