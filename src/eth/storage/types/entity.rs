use crate::eth::storage::MinedPointInTime;
use crate::eth::storage::StorageError;
use crate::eth::storage::StratusStorage;
use crate::eth::types::Account;
use crate::eth::types::Address;
use crate::eth::types::Slot;
use crate::eth::types::SlotIndex;

/// Abstraction over address-keyed ([`Account`]) and slot-keyed ([`Slot`]) reads
pub trait EntityRead: Sized + Clone {
    type Key: Copy;
    /// Reads the latest (mined tip) value from the cache, if present.
    fn read_latest_cache(s: &StratusStorage, key: &Self::Key) -> Option<Self>;
    /// Tries to read from the latest cache, if it gets a lock contention error from the cache, returns None
    fn try_read_latest_cache(s: &StratusStorage, key: &Self::Key) -> Option<Self>;
    /// Retains only the keys that are missing from both the temporary storage and the latest cache.
    fn retain_missing_keys(s: &StratusStorage, keys: &mut Vec<Self::Key>);
    /// Reads from temporary (pending) storage.
    fn read_temp(s: &StratusStorage, key: Self::Key) -> Option<Self>;
    /// Reads from permanent storage at the resolved mined point.
    fn read_perm(s: &StratusStorage, key: Self::Key, point: MinedPointInTime<'_>) -> Result<Self, StorageError>;
    /// Caches the value as a latest (mined tip) entry, if not already cached.
    fn cache_latest_if_missing(s: &StratusStorage, key: Self::Key, value: Self);
}

impl EntityRead for Account {
    type Key = Address;

    fn read_temp(s: &StratusStorage, address: Address) -> Option<Self> {
        s.temp.read_account(address)
    }

    fn read_latest_cache(s: &StratusStorage, address: &Address) -> Option<Self> {
        s.cache.get_account_latest(address)
    }

    fn try_read_latest_cache(s: &StratusStorage, address: &Address) -> Option<Self> {
        s.cache.try_get_account_latest(address).ok().flatten()
    }

    fn retain_missing_keys(s: &StratusStorage, keys: &mut Vec<Self::Key>) {
        s.temp.transaction_storage.retain_missing_accounts(keys);
        keys.retain(|address| !s.cache.contains_account(address));
    }

    fn read_perm(s: &StratusStorage, address: Address, point: MinedPointInTime<'_>) -> Result<Self, StorageError> {
        s.perm
            .read_account(address, &point)
            .map(|acc_opt| acc_opt.unwrap_or_else(|| Account::new_empty(address)))
    }

    fn cache_latest_if_missing(s: &StratusStorage, address: Address, account: Self) {
        s.cache.cache_account_latest_if_missing(address, account);
    }
}

impl EntityRead for Slot {
    type Key = (Address, SlotIndex);

    fn read_temp(s: &StratusStorage, (address, index): (Address, SlotIndex)) -> Option<Self> {
        s.temp.read_slot(address, index)
    }

    fn read_latest_cache(s: &StratusStorage, key: &(Address, SlotIndex)) -> Option<Self> {
        let (address, index) = key;
        s.cache.get_slot_latest(address, index)
    }

    fn try_read_latest_cache(s: &StratusStorage, key: &Self::Key) -> Option<Self> {
        let (address, index) = key;
        s.cache.try_get_slot_latest(address, index).ok().flatten()
    }

    fn retain_missing_keys(s: &StratusStorage, keys: &mut Vec<Self::Key>) {
        s.temp.transaction_storage.retain_missing_slots(keys);
        keys.retain(|(address, index)| !s.cache.contains_slot(address, index));
    }

    fn read_perm(s: &StratusStorage, key: (Address, SlotIndex), point: MinedPointInTime<'_>) -> Result<Self, StorageError> {
        let (address, index) = key;
        s.perm
            .read_slot(address, index, &point)
            .map(|slot_opt| slot_opt.unwrap_or_else(|| Slot::new_empty(index)))
    }

    fn cache_latest_if_missing(s: &StratusStorage, key: (Address, SlotIndex), slot: Self) {
        let (address, index) = key;
        s.cache.cache_slot_latest_if_missing(address, index, slot.value);
    }
}
