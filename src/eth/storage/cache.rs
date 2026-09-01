use std::hash::Hash;
use std::time::Duration;

use clap::Parser;
use display_json::DebugAsJson;
use indexmap::Equivalent;
use quick_cache::UnitWeighter;
use quick_cache::sync::Cache;
use quick_cache::sync::GuardResult;

use crate::eth::executor::State;
use crate::eth::executor::types::state::Change;
use crate::eth::executor::types::state::Complete;
use crate::eth::types::Account;
use crate::eth::types::Address;
use crate::eth::types::Slot;
use crate::eth::types::SlotIndex;
use crate::eth::types::SlotValue;

pub struct StorageCache {
    account_latest_cache: Cache<Address, Account, UnitWeighter>,
    slot_latest_cache: Cache<(Address, SlotIndex), SlotValue, UnitWeighter>,
}

#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct CacheConfig {
    /// Capacity of account history cache
    #[arg(long = "account-history-cache-capacity", env = "ACCOUNT_HISTORY_CACHE_CAPACITY", default_value = "20000")]
    pub account_history_cache_capacity: usize,

    /// Capacity of slot history cache
    #[arg(long = "slot-history-cache-capacity", env = "SLOT_HISTORY_CACHE_CAPACITY", default_value = "100000")]
    pub slot_history_cache_capacity: usize,
}

impl CacheConfig {
    pub fn init(&self) -> StorageCache {
        StorageCache::new(self)
    }
}

impl StorageCache {
    pub fn new(config: &CacheConfig) -> Self {
        Self {
            account_latest_cache: Cache::with_weighter(
                config.account_history_cache_capacity,
                config.account_history_cache_capacity as u64,
                UnitWeighter,
            ),
            slot_latest_cache: Cache::with_weighter(config.slot_history_cache_capacity, config.slot_history_cache_capacity as u64, UnitWeighter),
        }
    }

    pub fn clear(&self) {
        self.account_latest_cache.clear();
        self.slot_latest_cache.clear();
    }

    fn _cache_account_and_slots_from_changes_impl(
        changes: State<Complete>,
        account_cache: &Cache<Address, Account, UnitWeighter>,
        slot_cache: &Cache<(Address, SlotIndex), SlotValue, UnitWeighter>,
    ) {
        // cache accounts
        for (address, change) in changes.accounts.into_iter() {
            let account = change.to_account(address);
            account_cache.insert(address, account);
        }

        // cache slots
        for ((address, index), value) in changes.slots.into_iter() {
            slot_cache.insert((address, index), value.take_value());
        }
    }

    pub fn cache_account_and_slots_latest_from_changes(&self, changes: State<Complete>) {
        Self::_cache_account_and_slots_from_changes_impl(changes, &self.account_latest_cache, &self.slot_latest_cache);
    }

    pub fn cache_account_latest_if_missing(&self, address: Address, account: Account) {
        self.account_latest_cache.insert_if_missing(address, account);
    }

    pub fn cache_slot_latest_if_missing(&self, address: Address, slot: Slot) {
        self.slot_latest_cache.insert_if_missing((address, slot.index), slot.value);
    }

    pub fn get_account_latest(&self, address: &Address) -> Option<Account> {
        self.account_latest_cache.get(address)
    }

    pub fn get_slot_latest(&self, address: &Address, index: &SlotIndex) -> Option<Slot> {
        self.slot_latest_cache
            .get(&SlotKeyRef(address, index))
            .map(|value| Slot { value, index: *index })
    }

    pub fn contains_account(&self, address: &Address) -> bool {
        self.account_latest_cache.contains_key(address)
    }

    pub fn contains_slot(&self, address: &Address, index: &SlotIndex) -> bool {
        self.slot_latest_cache.contains_key(&SlotKeyRef(address, index))
    }
}

// Borrowed lookup key for `slot_latest_cache`. `std` provides no `Borrow<(&A, &B)>` impl for `(A, B)`, so a tuple of
// references cannot be used directly with `get`/`contains_key`; and the orphan rule (E0117) forbids implementing
// `Equivalent` for `(&Address, &SlotIndex)` directly because tuples are always foreign. The derived `Hash` hashes the
// fields in the same order as the `(Address, SlotIndex)` tuple, which is required for the lookups to match.
#[derive(Hash)]
struct SlotKeyRef<'a>(&'a Address, &'a SlotIndex);

impl Equivalent<(Address, SlotIndex)> for SlotKeyRef<'_> {
    fn equivalent(&self, key: &(Address, SlotIndex)) -> bool {
        self.0 == &key.0 && self.1 == &key.1
    }
}

trait CacheExt<Key, Val> {
    fn insert_if_missing(&self, key: Key, val: Val);
}

impl<Key, Val, We, B, L> CacheExt<Key, Val> for Cache<Key, Val, We, B, L>
where
    Key: Hash + Equivalent<Key> + ToOwned<Owned = Key> + std::cmp::Eq,
    Val: Clone,
    We: quick_cache::Weighter<Key, Val> + Clone,
    B: std::hash::BuildHasher + Clone,
    L: quick_cache::Lifecycle<Key, Val> + Clone,
{
    fn insert_if_missing(&self, key: Key, val: Val) {
        // None means wait forever, if someone else has the guard it will block.
        // Some(Duration::ZERO) means "if someone has the guard return immediately"
        // since we're only inserting the value if it is not cached yet the latter
        // is the desired behavior.
        match self.get_value_or_guard(&key, Some(Duration::ZERO)) {
            GuardResult::Value(_) | GuardResult::Timeout => (),
            GuardResult::Guard(g) => {
                // this fails if an unguarded insert already inserted to this key
                let _ = g.insert(val);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_cache() -> StorageCache {
        CacheConfig {
            account_history_cache_capacity: 16,
            slot_history_cache_capacity: 16,
        }
        .init()
    }

    #[test]
    fn contains_slot_finds_cached_slots() {
        let cache = new_cache();
        let address = Address::new([0xAA; 20]);
        let index = SlotIndex::from([7u64, 0, 0, 0]);
        let other_index = SlotIndex::from([8u64, 0, 0, 0]);
        let other_address = Address::new([0xBB; 20]);

        cache.cache_slot_latest_if_missing(address, Slot::new(index, SlotValue::from([42u64, 0, 0, 0])));

        let expected_slot = Slot::new(index, SlotValue::from([42u64, 0, 0, 0]));
        assert_eq!(cache.get_slot_latest(&address, &index), Some(expected_slot));
        assert_eq!(cache.get_slot_latest(&address, &other_index), None);
        assert_eq!(cache.get_slot_latest(&other_address, &index), None);

        assert!(cache.contains_slot(&address, &index));
        assert!(!cache.contains_slot(&address, &other_index));
        assert!(!cache.contains_slot(&other_address, &index));
    }
}
