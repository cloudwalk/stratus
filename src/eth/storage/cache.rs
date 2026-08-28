use std::hash::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use clap::Parser;
use display_json::DebugAsJson;
use parking_lot::Mutex;
use tinyufo::TinyUfo;

use crate::eth::executor::State;
use crate::eth::executor::types::state::Complete;
use crate::eth::types::Account;
use crate::eth::types::Address;
use crate::eth::types::Slot;
use crate::eth::types::SlotIndex;
use crate::eth::types::SlotValue;

const INSERT_LOCK_SHARDS: usize = 64;

type VersionedKey<K> = (u64, K);

pub struct StorageCache {
    account_latest_cache: TinyUfo<VersionedKey<Address>, Account>,
    slot_latest_cache: TinyUfo<VersionedKey<(Address, SlotIndex)>, SlotValue>,
    generation: AtomicU64,
    insert_locks: [Mutex<()>; INSERT_LOCK_SHARDS],
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
            account_latest_cache: TinyUfo::new(config.account_history_cache_capacity, config.account_history_cache_capacity),
            slot_latest_cache: TinyUfo::new(config.slot_history_cache_capacity, config.slot_history_cache_capacity),
            generation: AtomicU64::new(0),
            insert_locks: std::array::from_fn(|_| Mutex::new(())),
        }
    }

    pub fn clear(&self) {
        // TinyUFO does not expose a clear operation. Moving to a new key generation
        // makes all existing entries inaccessible; they are reclaimed by normal eviction.
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    fn cache_account_and_slots_from_changes_impl(&self, changes: &State<Complete>) {
        // cache accounts
        for (address, change) in changes.accounts.iter() {
            let _guard = self.insert_lock(address);
            let key = (self.generation(), *address);
            let account = change.clone().to_account(*address);
            let _ = self.account_latest_cache.put(key, account, 1);
        }

        // cache slots
        for ((address, index), value) in changes.slots.iter() {
            let cache_key = (*address, *index);
            let _guard = self.insert_lock(&cache_key);
            let key = (self.generation(), cache_key);
            let _ = self.slot_latest_cache.put(key, *value.value(), 1);
        }
    }

    pub fn cache_account_and_slots_latest_from_changes(&self, changes: &State<Complete>) {
        self.cache_account_and_slots_from_changes_impl(changes);
    }

    pub fn cache_account_latest_if_missing(&self, address: Address, account: Account) {
        let _guard = self.insert_lock(&address);
        let key = (self.generation(), address);
        if self.account_latest_cache.get(&key).is_none() {
            let _ = self.account_latest_cache.put(key, account, 1);
        }
    }

    pub fn cache_slot_latest_if_missing(&self, address: Address, slot: Slot) {
        let cache_key = (address, slot.index);
        let _guard = self.insert_lock(&cache_key);
        let key = (self.generation(), cache_key);
        if self.slot_latest_cache.get(&key).is_none() {
            let _ = self.slot_latest_cache.put(key, slot.value, 1);
        }
    }

    pub fn get_account_latest(&self, address: Address) -> Option<Account> {
        self.account_latest_cache.get(&(self.generation(), address))
    }

    pub fn get_slot_latest(&self, address: Address, index: SlotIndex) -> Option<Slot> {
        self.slot_latest_cache
            .get(&(self.generation(), (address, index)))
            .map(|value| Slot { value, index })
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    fn insert_lock<Key: Hash>(&self, key: &Key) -> parking_lot::MutexGuard<'_, ()> {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let shard = hasher.finish() as usize % INSERT_LOCK_SHARDS;
        self.insert_locks[shard].lock()
    }
}
