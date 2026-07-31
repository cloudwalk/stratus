use clap::Parser;
use display_json::DebugAsJson;
use indexmap::Equivalent;
use quick_cache::UnitWeighter;
use quick_cache::sync::Cache;
use quick_cache::sync::DefaultLifecycle;
use quick_cache::sync::GuardResult;
use rustc_hash::FxBuildHasher;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;

/// Lower bound for the block hash cache, large enough to cover the block saver backlog of the
/// offline importer, which mines much further ahead than it saves.
pub const MIN_BLOCK_HASH_CACHE_CAPACITY: usize = 8192;

pub struct StorageCache {
    slot_cache: Cache<(Address, SlotIndex), SlotValue, UnitWeighter, FxBuildHasher>,
    account_cache: Cache<Address, Account, UnitWeighter, FxBuildHasher>,
    account_latest_cache: Cache<Address, Account, UnitWeighter, FxBuildHasher>,
    slot_latest_cache: Cache<(Address, SlotIndex), SlotValue, UnitWeighter, FxBuildHasher>,
    block_hash_cache: Cache<BlockNumber, Hash, UnitWeighter, FxBuildHasher>,
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

    /// Capacity of the block hash cache.
    ///
    /// Must hold the 256 blocks reachable by `BLOCKHASH` plus every block that was sealed but not
    /// saved yet, because mining and saving run in separate threads and an evicted hash that has
    /// not reached the permanent storage cannot be read back. Values below
    /// [`MIN_BLOCK_HASH_CACHE_CAPACITY`] are raised to it.
    #[arg(long = "block-hash-cache-capacity", env = "BLOCK_HASH_CACHE_CAPACITY", default_value_t = MIN_BLOCK_HASH_CACHE_CAPACITY)]
    pub block_hash_cache_capacity: usize,
}

impl CacheConfig {
    pub fn init(&self) -> StorageCache {
        StorageCache::new(self)
    }
}

impl StorageCache {
    pub fn new(config: &CacheConfig) -> Self {
        let block_hash_cache_capacity = config.block_hash_cache_capacity.max(MIN_BLOCK_HASH_CACHE_CAPACITY);

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
            block_hash_cache: Cache::with(
                block_hash_cache_capacity,
                block_hash_cache_capacity as u64,
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
        self.block_hash_cache.clear();
    }

    pub fn cache_slot_if_missing(&self, address: Address, slot: Slot) {
        self.slot_cache.insert_if_missing((address, slot.index), slot.value);
    }

    pub fn cache_account_if_missing(&self, account: Account) {
        self.account_cache.insert_if_missing(account.address, account);
    }

    fn _cache_account_and_slots_from_changes_impl(
        changes: ExecutionChanges,
        account_cache: &Cache<Address, Account, UnitWeighter, FxBuildHasher>,
        slot_cache: &Cache<(Address, SlotIndex), SlotValue, UnitWeighter, FxBuildHasher>,
    ) {
        // cache accounts
        for (address, change) in changes.accounts {
            let account = (address, change).into();
            account_cache.insert(address, account);
        }

        // cache slots
        for ((address, index), value) in changes.slots {
            slot_cache.insert((address, index), value);
        }
    }

    pub fn cache_account_and_slots_from_changes(&self, changes: ExecutionChanges) {
        Self::_cache_account_and_slots_from_changes_impl(changes, &self.account_cache, &self.slot_cache);
    }

    pub fn cache_account_and_slots_latest_from_changes(&self, changes: ExecutionChanges) {
        Self::_cache_account_and_slots_from_changes_impl(changes, &self.account_latest_cache, &self.slot_latest_cache);
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

    pub fn cache_block_hash(&self, number: BlockNumber, hash: Hash) {
        self.block_hash_cache.insert(number, hash);
    }

    pub fn cache_block_hash_if_missing(&self, number: BlockNumber, hash: Hash) {
        self.block_hash_cache.insert_if_missing(number, hash);
    }

    pub fn get_block_hash(&self, number: BlockNumber) -> Option<Hash> {
        self.block_hash_cache.get(&number)
    }
}

trait CacheExt<Key, Val> {
    fn insert_if_missing(&self, key: Key, val: Val);
}

impl<Key, Val, We, B, L> CacheExt<Key, Val> for Cache<Key, Val, We, B, L>
where
    Key: std::hash::Hash + Equivalent<Key> + ToOwned<Owned = Key> + std::cmp::Eq,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Blocks are sealed long before they are saved, so a configuration that undersizes the block
    /// hash cache would drop hashes that cannot be read back from the permanent storage yet.
    #[test]
    fn block_hash_cache_is_never_smaller_than_the_minimum() {
        let cache = CacheConfig {
            slot_cache_capacity: 1,
            account_cache_capacity: 1,
            account_history_cache_capacity: 1,
            slot_history_cache_capacity: 1,
            block_hash_cache_capacity: 1,
        }
        .init();

        let hash_of = |number: u64| Hash::new([number as u8; 32]);
        for number in 0..MIN_BLOCK_HASH_CACHE_CAPACITY as u64 {
            cache.cache_block_hash(BlockNumber::from(number), hash_of(number));
        }

        assert_eq!(cache.get_block_hash(BlockNumber::ZERO), Some(hash_of(0)));
    }
}
