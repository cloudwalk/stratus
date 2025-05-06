use clap::Parser;
use display_json::DebugAsJson;
use quick_cache::sync::Cache;
use quick_cache::sync::DefaultLifecycle;
use quick_cache::UnitWeighter;
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
}

#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct CacheConfig {
    /// Capacity of slot cache
    #[arg(long = "slot-cache-capacity", env = "SLOT_CACHE_CAPACITY", default_value = "0")]
    pub slot_cache_capacity: usize,

    /// Capacity of account cache
    #[arg(long = "account-cache-capacity", env = "ACCOUNT_CACHE_CAPACITY", default_value = "0")]
    pub account_cache_capacity: usize,
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
        }
    }
    pub fn clear(&self) {
        self.slot_cache.clear();
        self.account_cache.clear();
    }

    pub fn cache_slot(&self, address: Address, slot: Slot) {
        self.slot_cache.insert((address, slot.index), slot.value);
    }

    pub fn cache_account(&self, account: Account) {
        self.account_cache.insert(account.address, account);
    }

    pub fn cache_account_and_slots_from_changes(&self, changes: ExecutionChanges) {
        for change in changes.into_values() {
            // cache slots
            for slot in change.slots.into_values().flat_map(|slot| slot.take()) {
                self.slot_cache.insert((change.address, slot.index), slot.value);
            }

            // cache account
            let mut account = AccountWithSlots::new(change.address);
            if let Some(nonce) = change.nonce.take_ref() {
                account.info.nonce = *nonce;
            }
            if let Some(balance) = change.balance.take_ref() {
                account.info.balance = *balance;
            }
            if let Some(Some(bytecode)) = change.bytecode.take_ref() {
                account.info.bytecode = Some(bytecode.clone());
            }
            self.account_cache.insert(change.address, account.info);
        }
    }

    pub fn get_slot(&self, address: Address, index: SlotIndex) -> Option<Slot> {
        self.slot_cache.get(&(address, index)).map(|value| Slot { value, index })
    }

    pub fn get_account(&self, address: Address) -> Option<Account> {
        self.account_cache.get(&address)
    }
}
