use std::fmt::Debug;
use std::ops::Deref;

use revm_state::EvmStorageSlot;

use crate::alias::RevmBytecode;
use crate::eth::executor::types::state::Complete;
use crate::eth::executor::types::state::Final;
use crate::eth::executor::types::state::Incomplete;
use crate::eth::executor::types::state::Stage;
use crate::eth::types::Account;
use crate::eth::types::Address;
use crate::eth::types::Nonce;
use crate::eth::types::SlotValue;
use crate::eth::types::Wei;
use crate::ext::InfallibleExt;

pub trait Change: Clone + Debug + PartialEq + Eq + Default + serde::Serialize {
    type Inner: Default;
    fn is_changed(&self) -> bool;
    fn take_value(self) -> Self::Inner;
    fn changed(self) -> Option<Self::Inner>;
    fn changed_ref(&self) -> Option<&Self::Inner>;
}

impl Change for SlotValue {
    type Inner = SlotValue;

    fn take_value(self) -> Self::Inner {
        self
    }

    fn changed(self) -> Option<Self::Inner> {
        Some(self)
    }

    fn changed_ref(&self) -> Option<&Self::Inner> {
        Some(self)
    }

    fn is_changed(&self) -> bool {
        true
    }
}

impl<T: Clone + Debug + PartialEq + Eq + Default + serde::Serialize> Change for CompleteValue<T> {
    type Inner = T;

    fn take_value(self) -> T {
        match self {
            Self::Changed(value) => value,
            Self::Original(value) => value,
        }
    }

    fn changed(self) -> Option<T> {
        match self {
            Self::Changed(value) => Some(value),
            Self::Original(_) => None,
        }
    }

    fn changed_ref(&self) -> Option<&T> {
        match self {
            Self::Changed(value) => Some(value),
            Self::Original(_) => None,
        }
    }

<<<<<<< HEAD:src/eth/executor/types/state.rs
    pub fn merge(&mut self, other: State<Complete>) {
        other.accounts.into_iter().for_each(|(address, changes)| self.insert_account(address, changes));
        other
            .slots
            .into_iter()
            .for_each(|((address, index), changes)| self.insert_slot(address, index, changes));
    }

    pub fn finalize(&self) -> State<Final> {
        let accounts = self
            .accounts
            .iter()
            .filter_map(|(address, account)| account.complete().map(|acc| ((*address), acc)))
            .collect();

        let slots = self
            .slots
            .iter()
            .filter(|(_, change)| change.is_changed())
            .map(|(slot_key, slot_value)| (*(slot_key), slot_value.clone().take_value()))
            .collect();
        State { accounts, slots }
=======
    fn is_changed(&self) -> bool {
        matches!(self, Self::Changed(_))
>>>>>>> 96818912 (small state refac):src/eth/executor/types/state/values.rs
    }
}

impl<T: Clone + Debug + PartialEq + Eq + Default + serde::Serialize> Change for IncompleteValue<T> {
    type Inner = T;

    fn take_value(self) -> T {
        match self {
            Self::Changed(value) => value,
            Self::Unset => T::default(),
        }
    }

    fn changed(self) -> Option<T> {
        match self {
            Self::Changed(value) => Some(value),
            Self::Unset => None,
        }
    }

    fn changed_ref(&self) -> Option<&T> {
        match self {
            Self::Changed(value) => Some(value),
            Self::Unset => None,
        }
    }

    fn is_changed(&self) -> bool {
        matches!(self, Self::Changed(_))
    }
}

/// Complete-stage field: a real value, either the original (untouched by the block) or changed by it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(fake::Dummy))]
pub enum CompleteValue<T>
where
    T: PartialEq + Eq + Default,
{
    Original(T),
    Changed(T),
}

impl<T: PartialEq + Eq + Default> Default for CompleteValue<T> {
    fn default() -> Self {
        Self::Original(T::default())
    }
}

impl<T: PartialEq + Eq + Default> Deref for CompleteValue<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Changed(inner) => inner,
            Self::Original(inner) => inner,
        }
    }
}

impl<T: Clone + Debug + PartialEq + Eq + Default + serde::Serialize> CompleteValue<T>
where
    T: PartialEq + Eq + Default,
{
    pub fn value(&self) -> &T {
        match self {
            Self::Changed(value) => value,
            Self::Original(value) => value,
        }
    }

    // A mege should never make a value state unchanged.
    pub fn merge(&mut self, change: Self) {
        if change.is_changed() {
            *self = change;
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

    pub fn from_diff(original: T, current: T) -> Self {
        match original == current {
            true => Self::Original(current),
            false => Self::Changed(current),
        }
    }
}

impl From<EvmStorageSlot> for CompleteValue<SlotValue> {
    fn from(value: EvmStorageSlot) -> Self {
        match value.is_changed() {
            true => Self::Changed(value.present_value.into()),
            false => Self::Original(value.present_value.into()),
        }
    }
}

/// Incomplete-stage field: either the block changed this value, or the original is not yet known
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub enum IncompleteValue<T> {
    Changed(T),
    #[default]
    Unset,
}

impl<T> IncompleteValue<T> {
    /// Resolves an incomplete field into a complete one, using `original` when the block did not touch it.
    pub fn complete(self, original: T) -> CompleteValue<T>
    where
        T: PartialEq + Eq + Default,
    {
        match self {
            Self::Changed(value) => CompleteValue::Changed(value),
            Self::Unset => CompleteValue::Original(original),
        }
    }
}

impl<T, U> From<Option<U>> for IncompleteValue<T>
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
pub struct AccountChanges<S: Stage> {
    pub nonce: S::AccountChangeField<Nonce>,
    pub balance: S::AccountChangeField<Wei>,
    pub bytecode: S::AccountChangeField<Option<RevmBytecode>>,
}

impl<S: Stage> AccountChanges<S> {
    pub fn is_changed(&self) -> bool {
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

impl AccountChanges<Incomplete> {
    /// Fills every unset field with its real original value, advancing to [`Full`].
    pub fn complete(self, original: Account) -> AccountChanges<Complete> {
        AccountChanges {
            nonce: self.nonce.complete(original.nonce),
            balance: self.balance.complete(original.balance),
            bytecode: self.bytecode.complete(original.bytecode),
        }
    }
}

impl AccountChanges<Complete> {
    /// Checks if account nonce, balance or bytecode were modified.
<<<<<<< HEAD:src/eth/executor/types/state.rs
    pub fn is_changed(&self) -> bool {
        self.nonce.is_changed() || self.balance.is_changed() || self.bytecode.is_changed()
    }

    pub fn complete(&self) -> Option<AccountChanges<Final>> {
=======
    pub fn complete(self) -> Option<AccountChanges<Final>> {
>>>>>>> 96818912 (small state refac):src/eth/executor/types/state/values.rs
        self.is_changed().then(|| AccountChanges {
            nonce: self.nonce.clone(),
            balance: self.balance.clone(),
            bytecode: self.bytecode.clone(),
        })
    }

    pub fn merge(&mut self, other: AccountChanges<Complete>) {
        self.nonce.merge(other.nonce);
        self.balance.merge(other.balance);
        self.bytecode.merge(other.bytecode);
    }

    pub(crate) fn apply_original(&mut self, original_account: Account) {
        self.nonce.apply_original(original_account.nonce);
        self.balance.apply_original(original_account.balance);
        self.bytecode.apply_original(original_account.bytecode);
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
            nonce: CompleteValue::from_diff(original.nonce.into(), changed.nonce.into()),
            balance: CompleteValue::from_diff(original.balance.into(), changed.balance.into()),
            bytecode: CompleteValue::from_diff(original.code.take(), changed.code),
        }
    }
}

impl<S: Stage> std::fmt::Debug for AccountChanges<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&serde_json::to_string(self).expect_infallible())
    }
}

#[cfg(test)]
impl fake::Dummy<fake::Faker> for AccountChanges<Complete> {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(faker: &fake::Faker, rng: &mut R) -> Self {
        Self {
            nonce: fake::Dummy::dummy_with_rng(faker, rng),
            balance: fake::Dummy::dummy_with_rng(faker, rng),
            bytecode: CompleteValue::default(),
        }
    }
}
