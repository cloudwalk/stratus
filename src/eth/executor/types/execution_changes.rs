use std::collections::HashMap;
use std::marker::PhantomData;

use serde_with::serde_as;

use crate::eth::executor::AccountChanges;
use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
use crate::eth::types::Account;
use crate::eth::types::Address;
use crate::eth::types::Slot;
use crate::eth::types::SlotIndex;
use crate::eth::types::SlotValue;
use crate::ext::OptionExt;

/// Stage marker: changes may still contain `Default` placeholders for fields the external block
/// did not touch. Must be [`ExecutionChanges::complete`]-d before consumption. (eg. on Block replication)
pub struct Incomplete;

/// Stage marker: every value is final (either changed by the block or filled with the original
/// account from perm). Safe to consume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Complete;

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct ExecutionChanges<Stage = Complete> {
    pub accounts: HashMap<Address, AccountChanges, hash_hasher::HashBuildHasher>,
    #[serde_as(as = "Vec<(_, _)>")]
    pub slots: HashMap<(Address, SlotIndex), SlotValue, hash_hasher::HashBuildHasher>,
    _stage: PhantomData<Stage>,
}

#[cfg(test)]
impl fake::Dummy<fake::Faker> for ExecutionChanges<Complete> {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(faker: &fake::Faker, rng: &mut R) -> Self {
        Self {
            accounts: fake::Dummy::dummy_with_rng(faker, rng),
            slots: fake::Dummy::dummy_with_rng(faker, rng),
            _stage: PhantomData,
        }
    }
}

/// Creates the INCOMPLETE account changes. Since if the bytecode/nonce/balance was not changed for
/// an account it is set to None, the resulting change is Self { changed: false, ..Default::default() }
/// operations that rely on knowing the original value (eg. updating the "latest" cache) can give wrong results.
impl From<BlockChangesRocksdb> for ExecutionChanges<Incomplete> {
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

        Self {
            accounts,
            slots,
            _stage: PhantomData,
        }
    }
}

/// Reads the original state of accounts from permanent storage.
pub trait AccountOriginalsReader {
    /// Returns the original accounts for the given addresses.
    fn read_accounts(&self, addresses: Vec<Address>) -> anyhow::Result<Vec<(Address, Account)>>;
}

impl ExecutionChanges<Incomplete> {
    /// Reads the original account state from `storage` and fills every `Default` placeholder,
    /// advancing to [`Complete`]. The only way to turn an `Incomplete` into `Complete`.
    pub fn complete(self, storage: &impl AccountOriginalsReader) -> anyhow::Result<ExecutionChanges<Complete>> {
        let addresses = self.accounts.keys().copied().collect::<Vec<_>>();
        let originals = storage.read_accounts(addresses)?;
        let mut accounts = self.accounts;
        for (address, original) in originals {
            match accounts.entry(address) {
                std::collections::hash_map::Entry::Occupied(mut entry) => entry.get_mut().apply_original(original),
                std::collections::hash_map::Entry::Vacant(_) => unreachable!("originals come from the changed accounts"),
            }
        }
        Ok(ExecutionChanges {
            accounts,
            slots: self.slots,
            _stage: PhantomData,
        })
    }
}

impl ExecutionChanges<Complete> {
    pub fn insert_account_changes(&mut self, address: Address, incoming_changes: AccountChanges) {
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

    pub fn merge(&mut self, other: ExecutionChanges<Complete>) {
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
