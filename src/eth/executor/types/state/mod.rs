pub mod values;

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt::Debug;

use foldhash::fast::RandomState;
use serde_with::serde_as;
pub use values::AccountChanges;
pub use values::Change;
pub use values::CompleteValue;
pub use values::IncompleteValue;

use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
use crate::eth::types::Account;
use crate::eth::types::Address;
use crate::eth::types::SlotIndex;
use crate::eth::types::SlotValue;
use crate::ext::OptionExt;

/// Stage marker: changes may be incomplete
#[derive(Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct Incomplete;

/// Stage marker: every value is final (either changed by the block or filled with the original
/// account from perm). Safe to consume.
#[derive(Clone, serde::Serialize, PartialEq, Eq, Default)]
pub struct Final;

/// Stage marker: may contain unchanged values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
pub struct Complete;

pub trait Stage: PartialEq + Eq + Default + Clone + serde::Serialize {
    type AccountChangeField<T: Clone + Debug + PartialEq + Eq + Default + serde::Serialize>: Change<Inner = T>;
    type SlotChangeField: Change<Inner = SlotValue>;
}

impl Stage for Incomplete {
    type AccountChangeField<T: Clone + Debug + PartialEq + Eq + Default + serde::Serialize> = IncompleteValue<T>;
    type SlotChangeField = SlotValue;
}

impl Stage for Final {
    type AccountChangeField<T: Clone + Debug + PartialEq + Eq + Default + serde::Serialize> = CompleteValue<T>;
    type SlotChangeField = SlotValue;
}

impl Stage for Complete {
    type AccountChangeField<T: Clone + Debug + PartialEq + Eq + Default + serde::Serialize> = CompleteValue<T>;
    type SlotChangeField = CompleteValue<SlotValue>;
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, Default)]
pub struct State<S: Stage> {
    pub accounts: HashMap<Address, AccountChanges<S>, RandomState>,
    #[serde_as(as = "Vec<(_, _)>")]
    pub slots: HashMap<(Address, SlotIndex), S::SlotChangeField, RandomState>,
}

#[cfg(test)]
impl fake::Dummy<fake::Faker> for State<Complete> {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(faker: &fake::Faker, rng: &mut R) -> Self {
        Self {
            accounts: fake::Dummy::dummy_with_rng(faker, rng),
            slots: fake::Dummy::dummy_with_rng(faker, rng),
        }
    }
}

/// Creates the INCOMPLETE account changes.
impl From<BlockChangesRocksdb> for State<Incomplete> {
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

impl State<Complete> {
    pub fn insert_account(&mut self, address: Address, incoming_changes: AccountChanges<Complete>) {
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

    pub fn insert_slot(&mut self, address: Address, index: SlotIndex, incoming_change: CompleteValue<SlotValue>) {
        match self.slots.entry((address, index)) {
            Entry::Occupied(mut entry) => entry.get_mut().merge(incoming_change),
            Entry::Vacant(entry) => {
                entry.insert(incoming_change);
            }
        }
    }

    pub fn insert_slots(&mut self, address: Address, slots: Vec<(SlotIndex, CompleteValue<SlotValue>)>) {
        slots.into_iter().for_each(|(index, change)| self.insert_slot(address, index, change));
    }

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
            .filter_map(|(address, account)| {
                if address.is_ignored() {
                    None
                } else {
                    account.complete().map(|acc| (*address, acc))
                }
            })
            .collect();

        let slots = self
            .slots
            .iter()
            .filter_map(|((address, index), slot_value)| {
                if address.is_ignored() {
                    None
                } else {
                    slot_value.changed_ref().copied().map(|value| ((*address, *index), value))
                }
            })
            .collect();
        State { accounts, slots }
    }
}

impl State<Incomplete> {
    /// Reads the original account state from `storage` and resolves every unset field, advancing to
    /// [`Full`]. The only way to turn an `Incomplete` into `Full`.
    ///
    /// Accounts not present in permanent storage (newly created by the block) resolve to
    /// `Account::default()`, which is their correct pre-state.
    pub fn complete(self, storage: &impl AccountOriginalsReader) -> anyhow::Result<State<Complete>> {
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

        let slots = self
            .slots
            .into_iter()
            .map(|(slot_key, slot_value)| (slot_key, CompleteValue::Changed(slot_value)))
            .collect();

        Ok(State { accounts, slots })
    }
}
