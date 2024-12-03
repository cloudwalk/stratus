use std::num::NonZeroUsize;

use lru::LruCache;
use parking_lot::Mutex;
use rustc_hash::FxBuildHasher;
use smallvec::SmallVec;

use super::AccountWithSlots;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;

pub struct StorageCache {
    slot_cache: Mutex<LruCache<(Address, SlotIndex), SlotValue, FxBuildHasher>>,
    account_cache: Mutex<LruCache<Address, Account, FxBuildHasher>>,
}

impl Default for StorageCache {
    fn default() -> Self {
        Self {
            slot_cache: Mutex::new(LruCache::with_hasher(NonZeroUsize::new(100_000).unwrap(), FxBuildHasher)),
            account_cache: Mutex::new(LruCache::with_hasher(NonZeroUsize::new(20_000).unwrap(), FxBuildHasher)),
        }
    }
}

impl StorageCache {
    pub fn clear(&self) {
        self.slot_cache.lock().clear();
        self.account_cache.lock().clear();
    }

    pub fn cache_slot(&self, address: Address, slot: Slot) {
        self.slot_cache.lock().put((address, slot.index), slot.value);
    }

    pub fn cache_account(&self, account: Account) {
        self.account_cache.lock().put(account.address, account);
    }

    pub fn cache_account_and_slots_from_changes(&self, changes: ExecutionChanges) {
        let mut slot_batch = SmallVec::<[_; 16]>::new();
        let mut account_batch = SmallVec::<[_; 8]>::new();

        for change in changes.into_values() {
            // cache slots
            for slot in change.slots.into_values().flat_map(|slot| slot.take()) {
                slot_batch.push(((change.address, slot.index), slot.value));
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
            account_batch.push((change.address, account.info));
        }

        {
            let mut slot_lock = self.slot_cache.lock();
            for (key, value) in slot_batch {
                slot_lock.push(key, value);
            }
        }
        {
            let mut account_lock = self.account_cache.lock();
            for (key, value) in account_batch {
                account_lock.push(key, value);
            }
        }
    }

    pub fn get_slot(&self, address: Address, index: SlotIndex) -> Option<Slot> {
        self.slot_cache.lock().get(&(address, index)).map(|&value| Slot { value, index })
    }

    pub fn get_account(&self, address: Address) -> Option<Account> {
        self.account_cache.lock().get(&address).cloned()
    }
}
