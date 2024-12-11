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

impl Default for StorageCache {
    fn default() -> Self {
        Self {
            slot_cache: Cache::with(100_000, 100_000, UnitWeighter, FxBuildHasher, DefaultLifecycle::default()),
            account_cache: Cache::with(20_000, 20_000, UnitWeighter, FxBuildHasher, DefaultLifecycle::default()),
        }
    }
}

impl StorageCache {
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
