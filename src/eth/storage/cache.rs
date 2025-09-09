use std::hash::Hash;

use clap::Parser;
use display_json::DebugAsJson;
use indexmap::Equivalent;
use quick_cache::UnitWeighter;
use quick_cache::sync::Cache;
use quick_cache::sync::DefaultLifecycle;
use quick_cache::sync::GuardResult;
use rustc_hash::FxBuildHasher;

use super::AccountWithSlots;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;

pub struct StorageCache {
    slot_cache: Cache<(Address, SlotIndex), SlotValue, UnitWeighter, FxBuildHasher>,
    account_cache: Cache<Address, Account, UnitWeighter, FxBuildHasher>,
    account_latest_cache: Cache<Address, Account, UnitWeighter, FxBuildHasher>,
    slot_latest_cache: Cache<(Address, SlotIndex), SlotValue, UnitWeighter, FxBuildHasher>,
}

#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct CacheConfig {
    /// Capacity of slot cache
    #[arg(long = "slot-cache-capacity", env = "SLOT_CACHE_CAPACITY", default_value = "100000")]
    pub slot_cache_capacity: usize,

    /// Capacity of account cache
    #[arg(long = "account-cache-capacity", env = "ACCOUNT_CACHE_CAPACITY", default_value = "20000")]
    pub account_cache_capacity: usize,

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
            slot_cache: Cache::with(
                config.slot_cache_capacity,
                config.slot_cache_capacity as u64,
                UnitWeighter,
                FxBuildHasher,
                DefaultLifecycle::default(),
            ),
            account_cache: Cache::with(
                config.account_cache_capacity,
                config.account_cache_capacity as u64,
                UnitWeighter,
                FxBuildHasher,
                DefaultLifecycle::default(),
            ),
            account_latest_cache: Cache::with(
                config.account_history_cache_capacity,
                config.account_history_cache_capacity as u64,
                UnitWeighter,
                FxBuildHasher,
                DefaultLifecycle::default(),
            ),
            slot_latest_cache: Cache::with(
                config.slot_history_cache_capacity,
                config.slot_history_cache_capacity as u64,
                UnitWeighter,
                FxBuildHasher,
                DefaultLifecycle::default(),
            ),
        }
    }

    pub fn clear(&self) {
        self.slot_cache.clear();
        self.account_cache.clear();
        self.account_latest_cache.clear();
        self.slot_latest_cache.clear();
    }

    pub fn cache_slot_if_missing(&self, address: Address, slot: Slot) {
        self.slot_cache.insert_if_missing((address, slot.index), slot.value);
    }

    pub fn cache_account_if_missing(&self, account: Account) {
        self.account_cache.insert_if_missing(account.address, account);
    }

    pub fn _cache_changes_impl(
        slot_cache: &Cache<(Address, SlotIndex), SlotValue, UnitWeighter, FxBuildHasher>,
        account_cache: &Cache<Address, Account, UnitWeighter, FxBuildHasher>,
        changes: ExecutionChanges,
    ) {
        for (address, change) in changes {
            // cache slots
            for (slot_index, slot_value) in change.slots {
                slot_cache.insert((address, slot_index), slot_value);
            }

            // cache account
            let mut account = AccountWithSlots::new(address);
            if let Some(nonce) = change.nonce.take_ref() {
                account.info.nonce = *nonce;
            }
            if let Some(balance) = change.balance.take_ref() {
                account.info.balance = *balance;
            }
            if let Some(Some(bytecode)) = change.bytecode.take_ref() {
                account.info.bytecode = Some(bytecode.clone());
            }
            account_cache.insert(address, account.info);
        }
    }

    pub fn cache_account_and_slots_from_changes(&self, changes: ExecutionChanges) {
        Self::_cache_changes_impl(&self.slot_cache, &self.account_cache, changes);
    }

    pub fn cache_account_and_slots_latest_from_changes(&self, changes: ExecutionChanges) {
        Self::_cache_changes_impl(&self.slot_latest_cache, &self.account_latest_cache, changes);
    }

    pub fn get_slot(&self, address: Address, index: SlotIndex) -> Option<Slot> {
        self.slot_cache.get(&(address, index)).map(|value| Slot { value, index })
    }

    pub fn get_account(&self, address: Address) -> Option<Account> {
        self.account_cache.get(&address)
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
        match self.get_value_or_guard(&key, None) {
            GuardResult::Value(_) => (),
            GuardResult::Guard(g) => {
                // this fails if an unguarded insert already inserted to this key
                let _ = g.insert(val);
            }
            GuardResult::Timeout => unreachable!(),
        }
    }
}
