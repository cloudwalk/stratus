use std::hash::Hash;
use std::time::Duration;

use clap::Parser;
use display_json::DebugAsJson;
use foldhash::fast::FixedState;
use indexmap::Equivalent;
use quick_cache::UnitWeighter;
use quick_cache::sync::Cache;
use quick_cache::sync::DefaultLifecycle;
use quick_cache::sync::GuardResult;

use crate::eth::executor::State;
use crate::eth::executor::types::state::Change;
use crate::eth::executor::types::state::Complete;
use crate::eth::types::Account;
use crate::eth::types::Address;
use crate::eth::types::Slot;
use crate::eth::types::SlotIndex;
use crate::eth::types::SlotValue;

type CacheHasher = FixedState;

pub struct StorageCache {
    account_latest_cache: Cache<Address, Account, UnitWeighter, CacheHasher>,
    slot_latest_cache: Cache<(Address, SlotIndex), SlotValue, UnitWeighter, CacheHasher>,
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
            account_latest_cache: Cache::with(
                config.account_history_cache_capacity,
                config.account_history_cache_capacity as u64,
                UnitWeighter,
                CacheHasher::default(),
                DefaultLifecycle::default(),
            ),
            slot_latest_cache: Cache::with(
                config.slot_history_cache_capacity,
                config.slot_history_cache_capacity as u64,
                UnitWeighter,
                CacheHasher::default(),
                DefaultLifecycle::default(),
            ),
        }
    }

    pub fn clear(&self) {
        self.account_latest_cache.clear();
        self.slot_latest_cache.clear();
    }

    fn _cache_account_and_slots_from_changes_impl(
        changes: State<Complete>,
        account_cache: &Cache<Address, Account, UnitWeighter, FxBuildHasher>,
        slot_cache: &Cache<(Address, SlotIndex), SlotValue, UnitWeighter, FxBuildHasher>,
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

    pub fn get_account_latest(&self, address: Address) -> Option<Account> {
        self.account_latest_cache.get(&address)
    }

    pub fn get_slot_latest(&self, address: Address, index: SlotIndex) -> Option<Slot> {
        self.slot_latest_cache.get(&(address, index)).map(|value| Slot { value, index })
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
