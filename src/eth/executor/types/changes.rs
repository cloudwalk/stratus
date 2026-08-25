use std::collections::HashMap;
use std::fmt::Debug;
use std::ops::Deref;

use serde_with::serde_as;

use crate::alias::RevmBytecode;
use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
use crate::eth::types::Account;
use crate::eth::types::Address;
use crate::eth::types::Nonce;
use crate::eth::types::Slot;
use crate::eth::types::SlotIndex;
use crate::eth::types::SlotValue;
use crate::eth::types::Wei;
use crate::ext::InfallibleExt;
use crate::ext::OptionExt;

/// Stage marker: changes may be incomplete
#[derive(serde::Serialize)]
pub struct Incomplete;

/// Stage marker: every value is final (either changed by the block or filled with the original
/// account from perm). Safe to consume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
pub struct Complete;

pub trait Stage: serde::Serialize {
    type ChangeField<T: Clone + Debug + PartialEq + Eq + Default + serde::Serialize>: Clone + Debug + PartialEq + Eq + Default + serde::Serialize;
}

impl Stage for Incomplete {
    type ChangeField<T: Clone + Debug + PartialEq + Eq + Default + serde::Serialize> = IncompleteValue<T>;
}

impl Stage for Complete {
    type ChangeField<T: Clone + Debug + PartialEq + Eq + Default + serde::Serialize> = CompleteValue<T>;
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, Default)]
pub struct Changes<S: Stage = Complete> {
    pub accounts: HashMap<Address, AccountChanges<S>, hash_hasher::HashBuildHasher>,
    #[serde_as(as = "Vec<(_, _)>")]
    pub slots: HashMap<(Address, SlotIndex), SlotValue, hash_hasher::HashBuildHasher>,
}

#[cfg(test)]
impl fake::Dummy<fake::Faker> for Changes<Complete> {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(faker: &fake::Faker, rng: &mut R) -> Self {
        Self {
            accounts: fake::Dummy::dummy_with_rng(faker, rng),
            slots: fake::Dummy::dummy_with_rng(faker, rng),
        }
    }
}

/// Creates the INCOMPLETE account changes.
impl From<BlockChangesRocksdb> for Changes<Incomplete> {
    fn from(value: BlockChangesRocksdb) -> Self {
        let accounts = value
            .account_changes
            .into_iter()
            .map(|(address, changes)| {
                (
                    address.into(),
                    AccountChanges {
                        nonce: changes.nonce.into(),
                        balance: changes.balance.into(),
                        bytecode: changes.bytecode.map(|inner| inner.map_into()).into(),
                    },
                )
            })
            .collect();
        let slots = value
            .slot_changes
            .into_iter()
            .map(|((addr, idx), value)| ((addr.into(), idx.into()), value.into()))
            .collect();

        Self { accounts, slots }
    }
}

/// Reads the original state of accounts from permanent storage.
pub trait AccountOriginalsReader {
    /// Returns the original accounts for the given addresses.
    fn read_accounts(&self, addresses: Vec<Address>) -> anyhow::Result<Vec<(Address, Account)>>;
}

impl Changes<Incomplete> {
    /// Reads the original account state from `storage` and resolves every unset field, advancing to
    /// [`Complete`]. The only way to turn an `Incomplete` into `Complete`.
    ///
    /// Accounts not present in permanent storage (newly created by the block) resolve to
    /// `Account::default()`, which is their correct pre-state.
    pub fn complete(self, storage: &impl AccountOriginalsReader) -> anyhow::Result<Changes<Complete>> {
        let addresses = self.accounts.keys().copied().collect::<Vec<_>>();
        let original_accounts: HashMap<Address, Account> = storage.read_accounts(addresses)?.into_iter().collect();

        let accounts = self
            .accounts
            .into_iter()
            .map(|(address, changes)| {
                let original = original_accounts.get(&address).cloned().unwrap_or_default();
                (address, changes.complete(original))
            })
            .collect();
        Ok(Changes { accounts, slots: self.slots })
    }
}

impl Changes<Complete> {
    pub fn insert_account_changes(&mut self, address: Address, incoming_changes: AccountChanges<Complete>) {
        match self.accounts.entry(address) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let existing_changes = entry.get_mut();
                existing_changes.merge(incoming_changes);
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(incoming_changes);
            }
        }
    }

    pub fn insert_slot_changes(&mut self, address: Address, slots: Vec<Slot>) {
        for slot in slots {
            self.slots.insert((address, slot.index), slot.value);
        }
    }

    pub fn merge(&mut self, other: Changes<Complete>) {
        for (address, changes) in other.accounts {
            match self.accounts.entry(address) {
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    let current_changes = entry.get_mut();
                    current_changes.merge(changes);
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(changes);
                }
            }
        }
        self.slots.extend(other.slots);
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

impl<T> CompleteValue<T>
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
pub struct AccountChanges<S: Stage = Complete> {
    pub nonce: S::ChangeField<Nonce>,
    pub balance: S::ChangeField<Wei>,
    pub bytecode: S::ChangeField<Option<RevmBytecode>>,
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
