use parking_lot::RwLockReadGuard;
use tracing::Span;

use super::InMemoryTemporaryStorage;
use super::RocksPermanentStorage;
use super::StorageCache;
use super::permanent::rocks::types::BlockRocksdb;
#[cfg(feature = "dev")]
use crate::eth::genesis::GenesisConfig;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
#[cfg(feature = "dev")]
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMessage;
#[cfg(feature = "dev")]
use crate::eth::primitives::Nonce;
use crate::eth::primitives::PendingBlock;
use crate::eth::primitives::PendingBlockHeader;
use crate::eth::primitives::PointInTime;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
#[cfg(feature = "dev")]
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::StorageError;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionStage;
#[cfg(feature = "dev")]
use crate::eth::primitives::Wei;
#[cfg(feature = "dev")]
use crate::eth::primitives::test_accounts;
use crate::eth::storage::ReadKind;
use crate::eth::storage::TxCount;
use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
use crate::ext::not;
use crate::infra::metrics;
use crate::infra::metrics::timed;
use crate::infra::tracing::SpanExt;

mod label {
    pub(super) const TEMP: &str = "temporary";
    pub(super) const PERM: &str = "permanent";
    pub(super) const CACHE: &str = "cache";
}

/// Proxy that simplifies interaction with permanent and temporary storages.
///
/// Additionaly it tracks metrics that are independent of the storage implementation.
pub struct StratusStorage {
    temp: InMemoryTemporaryStorage,
    cache: StorageCache,
    pub perm: RocksPermanentStorage,
    // CONTRACT: Always acquire a lock when reading slots or accounts from latest (cache OR perm) and when saving a block
    transient_state_lock: parking_lot::RwLock<()>,
    #[cfg(feature = "dev")]
    perm_config: crate::eth::storage::permanent::PermanentStorageConfig,
}

pub use resolve::MinedPointInTime;

/// Pending-state resolution
mod resolve {
    use super::Account;
    use super::Address;
    use super::BlockNumber;
    use super::PointInTime;
    use super::ReadKind;
    use super::RwLockReadGuard;
    use super::Slot;
    use super::SlotIndex;
    use super::StorageError;
    use super::StratusStorage;
    use super::TxCount;
    use super::label;

    /// Prevents construction of [`MinedPointInTime`] outside this module.
    /// `Seal` is public (so the enum variants can be pattern-matched) but cannot be constructed
    /// externally because its field type is private.
    #[derive(Debug)]
    pub struct Seal(SealPrivate);

    #[derive(Debug)]
    struct SealPrivate;

    /// A [`PointInTime`] that has been resolved past the pending case.
    ///
    /// `Latest` carries an optional `transient_state_lock` read guard held across the latest read.
    ///
    /// The [`Seal`] field makes both variants impossible to construct outside this module,
    /// while still allowing pattern matching externally.
    #[derive(Debug)]
    pub enum MinedPointInTime<'a> {
        Latest(Seal, Option<RwLockReadGuard<'a, ()>>),
        Past(Seal, BlockNumber),
    }

    impl<'a> MinedPointInTime<'a> {
        fn mined(guard: Option<RwLockReadGuard<'a, ()>>) -> Self {
            Self::Latest(Seal(SealPrivate), guard)
        }

        fn mined_past(number: BlockNumber) -> Self {
            Self::Past(Seal(SealPrivate), number)
        }

        /// Returns the [`PointInTime`] this resolved point corresponds to.
        pub fn as_point_in_time(&self) -> PointInTime {
            match self {
                Self::Latest(_, _) => PointInTime::Mined,
                Self::Past(_, number) => PointInTime::MinedPast(*number),
            }
        }

        /// Returns `true` if this is the `Mined` (latest) point.
        pub fn is_mined(&self) -> bool {
            matches!(self, Self::Latest(_, _))
        }

        /// Extracts the read guard if present, leaving `Mined(None)` in its place.
        fn take_guard(&mut self) -> Option<RwLockReadGuard<'a, ()>> {
            match self {
                Self::Latest(_, guard) => guard.take(),
                Self::Past(_, _) => None,
            }
        }
    }

    /// Unlocks the guard fairly when dropped.
    impl<'a> Drop for MinedPointInTime<'a> {
        fn drop(&mut self) {
            if let Some(guard) = self.take_guard() {
                RwLockReadGuard::unlock_fair(guard);
            }
        }
    }

    impl<'a> std::fmt::Display for MinedPointInTime<'a> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            self.as_point_in_time().fmt(f)
        }
    }

    /// Outcome of resolving pending state for a read.
    #[derive(Debug)]
    pub(super) enum Resolved<'a, T> {
        /// Found in the pending cache.
        PendingCache(T),
        /// Found in temporary storage.
        Temp(T),
        /// Nothing pending.
        Miss(MinedPointInTime<'a>),
    }

    impl StratusStorage {
        /// Resolves pending state for a slot read.
        /// Returns [`Resolved::Miss`] if no valid pending slot was found.
        pub(super) fn resolve_slot(
            &self,
            point_in_time: PointInTime,
            address: Address,
            index: SlotIndex,
            kind: ReadKind,
        ) -> Result<Resolved<'_, Slot>, StorageError> {
            // MinedPast is historical and immutable
            if let PointInTime::MinedPast(number) = point_in_time {
                return Ok(Resolved::Miss(MinedPointInTime::mined_past(number)));
            }

            if point_in_time == PointInTime::Pending {
                if matches!(kind, ReadKind::Transaction)
                    && let Some(slot) = self._read_slot_pending_cache(address, index)
                {
                    return Ok(Resolved::PendingCache(slot));
                }
                if let Some(slot) = self._read_slot_temp(address, index, kind)? {
                    tracing::debug!(storage = %label::TEMP, %address, %index, value = %slot.value, "slot found in temporary storage");
                    return Ok(Resolved::Temp(slot));
                }
            }

            Ok(Resolved::Miss(self.resolve_mined_staleness(kind)))
        }

        /// Resolves pending state for an account read.
        /// Returns [`Resolved::Miss`] if no valid pending account was found.
        pub(super) fn resolve_account(&self, point_in_time: PointInTime, address: Address, kind: ReadKind) -> Result<Resolved<'_, Account>, StorageError> {
            if let PointInTime::MinedPast(number) = point_in_time {
                return Ok(Resolved::Miss(MinedPointInTime::mined_past(number)));
            }

            if point_in_time == PointInTime::Pending {
                if matches!(kind, ReadKind::Transaction)
                    && let Some(account) = self._read_account_pending_cache(address)
                {
                    return Ok(Resolved::PendingCache(account));
                }
                if let Some(account) = self._read_account_temp(address, kind)? {
                    tracing::debug!(storage = %label::TEMP, %address, ?account, "account found in temporary storage");
                    return Ok(Resolved::Temp(account));
                }
            }
            Ok(Resolved::Miss(self.resolve_mined_staleness(kind)))
        }

        /// Determines the mined point-in-time and whether a read guard is needed for a `Call` kind.
        /// The mined state `(<latest block>, TxCount::Full)` is stale for a call if it is newer (gt)
        /// than the state the call is running on (`(BlockNumber, TxCount)`).
        fn resolve_mined_staleness(&self, kind: ReadKind) -> MinedPointInTime<'_> {
            match kind {
                ReadKind::Call((block_number, tx_count)) => {
                    let guard = self.transient_state_lock.read();
                    let mined = self.read_mined_block_number();
                    if (block_number, tx_count) >= (mined, TxCount::Full) {
                        MinedPointInTime::mined(Some(guard))
                    } else {
                        drop(guard);
                        let target = match tx_count {
                            TxCount::Partial(_) => block_number.prev().unwrap_or_default(),
                            TxCount::Full => block_number,
                        };
                        MinedPointInTime::mined_past(target)
                    }
                }
                _ => MinedPointInTime::mined(None),
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::super::StratusStorage;
        use crate::eth::primitives::Address;
        use crate::eth::primitives::BlockNumber;
        use crate::eth::primitives::PointInTime;
        use crate::eth::primitives::SlotIndex;
        use crate::eth::storage::ReadKind;
        use crate::eth::storage::TxCount;

        #[test]
        fn pending_partial_call_latest_becomes_stale_once_block_is_mined() {
            let storage = StratusStorage::new_test().expect("failed to build test storage");

            let address = Address::ZERO;
            let index = SlotIndex::ZERO;

            // Pending call: block_number is the pending block (mined + 1 = 6), pinned to tx 0.
            let mined_at_start = 5u64;
            let pending_block_number = BlockNumber::from(mined_at_start + 1);
            storage.set_mined_block_number(BlockNumber::from(mined_at_start));

            let kind = ReadKind::Call((pending_block_number, TxCount::Partial(0)));

            // At call start: block 6 is still pending (mined=5). Latest (block 5) is a safe base.
            // resolve_slot should return Miss(Mined(Some(guard))).
            let resolved = storage.resolve_slot(PointInTime::Pending, address, index, kind).expect("resolve_slot");
            match resolved {
                super::Resolved::Miss(mut point) => {
                    assert!(point.is_mined(), "should read latest mined base while block is still pending");
                    assert!(point.take_guard().is_some(), "guard should be held for valid latest read");
                }
                other => panic!("expected Miss, got {other:?}"),
            }

            // The pending block is mined mid-call, advancing the mined tip to 6.
            storage.set_mined_block_number(pending_block_number);

            // The call is now stale: reading "latest" would observe block 6's aggregate state,
            // not the tx-0 base. resolve_slot should downgrade to MinedPast(5) = b.prev() with no guard.
            let resolved = storage.resolve_slot(PointInTime::Pending, address, index, kind).expect("resolve_slot");
            match resolved {
                super::Resolved::Miss(mut point) => {
                    assert!(!point.is_mined(), "stale call should not read latest");
                    assert_eq!(
                        point.as_point_in_time(),
                        PointInTime::MinedPast(BlockNumber::from(mined_at_start)),
                        "stale Partial call should downgrade to MinedPast(b.prev())"
                    );
                    assert!(point.take_guard().is_none(), "no guard for historical read");
                }
                other => panic!("expected Miss, got {other:?}"),
            }
        }

        #[test]
        fn mined_full_call_downgrades_to_minedpast_block_not_prev() {
            let storage = StratusStorage::new_test().expect("failed to build test storage");

            let address = Address::ZERO;
            let index = SlotIndex::ZERO;

            // Mined Full call: block_number = 5, mined = 5 → valid (b >= mined).
            let call_block = BlockNumber::from(5u64);
            storage.set_mined_block_number(call_block);

            let kind = ReadKind::Call((call_block, TxCount::Full));

            let resolved = storage.resolve_slot(PointInTime::Mined, address, index, kind).expect("resolve_slot");
            match resolved {
                super::Resolved::Miss(mut point) => {
                    assert!(point.is_mined(), "Full call should read latest while block is the mined tip");
                    assert!(point.take_guard().is_some(), "guard should be held for valid latest read");
                }
                other => panic!("expected Miss, got {other:?}"),
            }

            // A newer block is mined mid-call, advancing the mined tip to 6.
            storage.set_mined_block_number(BlockNumber::from(6u64));

            // Stale: b=5 < mined=6. Full → MinedPast(5), NOT MinedPast(4).
            let resolved = storage.resolve_slot(PointInTime::Mined, address, index, kind).expect("resolve_slot");
            match resolved {
                super::Resolved::Miss(mut point) => {
                    assert!(!point.is_mined(), "stale call should not read latest");
                    assert_eq!(
                        point.as_point_in_time(),
                        PointInTime::MinedPast(call_block),
                        "stale Full call should downgrade to MinedPast(block_number), not prev()"
                    );
                    assert!(point.take_guard().is_none(), "no guard for historical read");
                }
                other => panic!("expected Miss, got {other:?}"),
            }
        }
    }
}

impl StratusStorage {
    /// Creates a new storage with the specified temporary and permanent implementations.
    pub fn new(
        temp: InMemoryTemporaryStorage,
        perm: RocksPermanentStorage,
        cache: StorageCache,
        #[cfg(feature = "dev")] perm_config: crate::eth::storage::permanent::PermanentStorageConfig,
    ) -> Result<Self, StorageError> {
        let this = Self {
            temp,
            cache,
            perm,
            transient_state_lock: parking_lot::RwLock::new(()),
            #[cfg(feature = "dev")]
            perm_config,
        };

        // create genesis block and accounts if necessary
        #[cfg(feature = "dev")]
        if !this.has_genesis()? {
            this.reset_to_genesis()?;
        }

        Ok(this)
    }

    /// Returns whether the genesis block exists
    pub fn has_genesis(&self) -> Result<bool, StorageError> {
        self.perm.has_genesis()
    }

    /// Clears the storage cache.
    pub fn clear_cache(&self) {
        tracing::info!("clearing storage cache");
        self.cache.clear();
    }

    #[cfg(test)]
    pub fn new_test() -> Result<Self, StorageError> {
        use tempfile::tempdir;

        use super::cache::CacheConfig;

        let temp = InMemoryTemporaryStorage::new(0.into());

        // Create a temporary directory for RocksDB
        let rocks_dir = tempdir().expect("Failed to create temporary directory for tests");
        let rocks_path_prefix = rocks_dir.path().to_str().unwrap().to_string();

        let perm = RocksPermanentStorage::new(
            Some(rocks_path_prefix.clone()),
            std::time::Duration::from_secs(240),
            super::permanent::RocksCfCacheConfig::default(),
            true,
            None,
            1024,
        )
        .expect("Failed to create RocksPermanentStorage for tests");

        let cache = CacheConfig {
            slot_cache_capacity: 100000,
            account_cache_capacity: 20000,
            account_history_cache_capacity: 20000,
            slot_history_cache_capacity: 100000,
        }
        .init();

        Self::new(
            temp,
            perm,
            cache,
            #[cfg(feature = "dev")]
            super::permanent::PermanentStorageConfig {
                rocks_path_prefix: Some(rocks_path_prefix),
                rocks_shutdown_timeout: std::time::Duration::from_secs(240),
                rocks_cf_cache: super::permanent::RocksCfCacheConfig::default(),
                rocks_disable_sync_write: false,
                rocks_cf_size_metrics_interval: None,
                genesis_file: crate::config::GenesisFileConfig::default(),
                rocks_file_descriptors_limit: 1024,
            },
        )
    }

    pub fn read_block_number_to_resume_import(&self) -> Result<BlockNumber, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_block_number_to_resume_import").entered();

        let number = self.read_pending_block_header().0.number;
        tracing::info!(?number, "got block number to resume import");

        Ok(number)
    }

    pub fn read_pending_block_header(&self) -> (PendingBlockHeader, TxCount) {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_pending_block_number").entered();
        tracing::debug!(storage = %label::TEMP, "reading pending block number");

        timed(|| self.temp.read_pending_block_header()).with(|m| {
            metrics::inc_storage_read_pending_block_number(m.elapsed, label::TEMP, true);
        })
    }

    pub fn read_mined_block_number(&self) -> BlockNumber {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_mined_block_number").entered();
        tracing::debug!(storage = %label::PERM, "reading mined block number");

        timed(|| self.perm.read_mined_block_number()).with(|m| {
            metrics::inc_storage_read_mined_block_number(m.elapsed, label::PERM, true);
        })
    }

    pub fn set_pending_from_external(&self, block: &ExternalBlock) {
        self.temp.set_pending_from_external(block);
    }

    pub fn set_mined_block_number(&self, block_number: BlockNumber) {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::set_mined_block_number", %block_number).entered();
        tracing::debug!(storage = %label::PERM, %block_number, "setting mined block number");

        timed(|| self.perm.set_mined_block_number(block_number)).with(|m| {
            metrics::inc_storage_set_mined_block_number(m.elapsed, label::PERM, true);
        });
    }

    // -------------------------------------------------------------------------
    // Accounts and slots
    // -------------------------------------------------------------------------

    pub fn save_accounts(&self, accounts: Vec<Account>) -> Result<(), StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::save_accounts").entered();

        // keep only accounts that does not exist in permanent storage
        let addresses: Vec<Address> = accounts.iter().map(|a| a.address).collect();
        let existing: std::collections::HashSet<Address> = self.perm.read_accounts(addresses)?.into_iter().map(|(addr, _)| addr).collect();
        let missing_accounts: Vec<Account> = accounts.into_iter().filter(|a| !existing.contains(&a.address)).collect();

        tracing::debug!(storage = %label::PERM, accounts = ?missing_accounts, "saving initial accounts");
        timed(|| self.perm.save_accounts(missing_accounts)).with(|m| {
            metrics::inc_storage_save_accounts(m.elapsed, label::PERM, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to save accounts");
            }
        })
    }

    fn _read_account_pending_cache(&self, address: Address) -> Option<Account> {
        timed(|| self.cache.get_account(address)).with(|m| {
            if m.result.is_some() {
                tracing::debug!(storage = %label::CACHE, %address, "account found in cache");
                metrics::inc_storage_read_account(m.elapsed, label::CACHE, PointInTime::Pending);
            }
        })
    }

    fn _read_account_latest_cache(&self, address: Address) -> Option<Account> {
        timed(|| self.cache.get_account_latest(address)).with(|m| {
            if m.result.is_some() {
                tracing::debug!(storage = %label::CACHE, %address, "account found in cache");
                metrics::inc_storage_read_account(m.elapsed, label::CACHE, PointInTime::Mined);
            }
        })
    }

    fn _read_account_temp(&self, address: Address, kind: ReadKind) -> Result<Option<Account>, StorageError> {
        tracing::debug!(storage = %label::TEMP, %address, "reading account");
        timed(|| self.temp.read_account(address, kind)).with(|m| {
            if m.result.as_ref().is_ok_and(|opt| opt.is_some()) {
                metrics::inc_storage_read_account(m.elapsed, label::TEMP, PointInTime::Pending);
            }
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to read account from temporary storage");
            }
        })
    }

    fn _read_account_perm(&self, address: Address, mined_point: resolve::MinedPointInTime<'_>) -> Result<Account, StorageError> {
        tracing::debug!(storage = %label::PERM, %address, "reading account");
        let account = timed(|| self.perm.read_account(address, &mined_point)).with(|m| {
            if m.result.as_ref().is_ok_and(|opt| opt.is_some()) {
                metrics::inc_storage_read_account(m.elapsed, label::PERM, mined_point.to_string());
            }
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to read account from permanent storage");
            }
        })?;
        Ok(match account {
            Some(account) => {
                tracing::debug!(storage = %label::PERM, %address, ?account, "account found in permanent storage");
                account
            }
            None => {
                tracing::debug!(storage = %label::PERM, %address, "account not found, assuming default value");
                Account::new_empty(address)
            }
        })
    }

    pub fn read_account(&self, address: Address, mut point_in_time: PointInTime, kind: ReadKind) -> Result<Account, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("storage::read_account", %address, %point_in_time).entered();

        let (account, found_in_perm) = 'query: {
            match self.resolve_account(point_in_time, address, kind)? {
                resolve::Resolved::PendingCache(account) => return Ok(account),
                resolve::Resolved::Temp(account) => break 'query (account, false),
                resolve::Resolved::Miss(mined_point) => {
                    // Mined (latest): try latest cache while guard is held, then fall through to perm.
                    if mined_point.is_mined() {
                        if let Some(account) = self._read_account_latest_cache(address) {
                            return Ok(account);
                        }
                    } else {
                        // Historical: update point_in_time for the caching match in the caller.
                        point_in_time = mined_point.as_point_in_time();
                    }
                    break 'query (self._read_account_perm(address, mined_point)?, true);
                }
            }
        };

        match (point_in_time, found_in_perm) {
            // Pending accounts found in the permanent storage (or not found in any storage) are always mined already
            (PointInTime::Pending, true) => {
                self.cache.cache_account_if_missing(account.clone());
                self.cache.cache_account_latest_if_missing(address, account.clone());
            }
            (PointInTime::Pending, false) => {
                self.cache.cache_account_if_missing(account.clone());
            }
            (PointInTime::Mined, _) => {
                self.cache.cache_account_latest_if_missing(address, account.clone());
            }
            _ => {}
        }
        Ok(account)
    }

    fn _read_slot_pending_cache(&self, address: Address, index: SlotIndex) -> Option<Slot> {
        timed(|| self.cache.get_slot(address, index)).with(|m| {
            if m.result.is_some() {
                tracing::debug!(storage = %label::CACHE, %address, slot = ?m.result, "slot found in cache");
                metrics::inc_storage_read_slot(m.elapsed, label::CACHE, PointInTime::Pending);
            }
        })
    }

    fn _read_slot_latest_cache(&self, address: Address, index: SlotIndex) -> Option<Slot> {
        timed(|| self.cache.get_slot_latest(address, index)).with(|m| {
            if m.result.is_some() {
                tracing::debug!(storage = %label::CACHE, %address, slot = ?m.result, "slot found in cache");
                metrics::inc_storage_read_slot(m.elapsed, label::CACHE, PointInTime::Mined);
            }
        })
    }

    fn _read_slot_temp(&self, address: Address, index: SlotIndex, kind: ReadKind) -> Result<Option<Slot>, StorageError> {
        tracing::debug!(storage = %label::TEMP, %address, %index, "reading slot");
        timed(|| self.temp.read_slot(address, index, kind)).with(|m| {
            if m.result.as_ref().is_ok_and(|opt| opt.is_some()) {
                metrics::inc_storage_read_slot(m.elapsed, label::TEMP, PointInTime::Pending);
            }
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to read slot from temporary storage");
            }
        })
    }

    fn _read_slot_perm(&self, address: Address, index: SlotIndex, mined_point: resolve::MinedPointInTime<'_>) -> Result<Slot, StorageError> {
        tracing::debug!(storage = %label::PERM, %address, %index, %mined_point, "reading slot");
        let slot = timed(|| self.perm.read_slot(address, index, &mined_point)).with(|m| {
            if m.result.as_ref().is_ok_and(|opt| opt.is_some()) {
                metrics::inc_storage_read_slot(m.elapsed, label::PERM, mined_point.to_string());
            }
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to read slot from permanent storage");
            }
        })?;
        Ok(match slot {
            Some(slot) => {
                tracing::debug!(storage = %label::PERM, %address, %index, ?slot, "slot found in permanent storage");
                slot
            }
            None => {
                tracing::debug!(storage = %label::PERM, %address, %index, "slot not found, assuming default value");
                Slot::new_empty(index)
            }
        })
    }

    pub fn read_slot(&self, address: Address, index: SlotIndex, mut point_in_time: PointInTime, kind: ReadKind) -> Result<Slot, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("storage::read_slot", %address, %index, %point_in_time).entered();

        let (slot, found_in_perm) = 'query: {
            match self.resolve_slot(point_in_time, address, index, kind)? {
                resolve::Resolved::PendingCache(slot) => return Ok(slot),
                resolve::Resolved::Temp(slot) => break 'query (slot, false),
                resolve::Resolved::Miss(mined_point) => {
                    // Mined (latest): try latest cache while guard is held, then fall through to perm.
                    if mined_point.is_mined() {
                        if let Some(slot) = self._read_slot_latest_cache(address, index) {
                            return Ok(slot);
                        }
                    } else {
                        // Historical: update point_in_time for the caching match in the caller.
                        point_in_time = mined_point.as_point_in_time();
                    }
                    break 'query (self._read_slot_perm(address, index, mined_point)?, true);
                }
            }
        };

        match (point_in_time, found_in_perm) {
            // Pending slots found in the permanent storage (or not found in any storage) are always mined already
            (PointInTime::Pending, true) => {
                self.cache.cache_slot_if_missing(address, slot);
                self.cache.cache_slot_latest_if_missing(address, slot);
            }
            (PointInTime::Pending, false) => {
                self.cache.cache_slot_if_missing(address, slot);
            }
            (PointInTime::Mined, _) => {
                self.cache.cache_slot_latest_if_missing(address, slot);
            }
            _ => {}
        }
        Ok(slot)
    }

    // -------------------------------------------------------------------------
    // Blocks
    // -------------------------------------------------------------------------

    pub fn save_execution(&self, tx: TransactionExecution) -> Result<(), StorageError> {
        let changes = tx.result.execution.changes.clone();

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::save_execution", tx_hash = %tx.info.hash).entered();
        tracing::debug!(storage = %label::TEMP, tx_hash = %tx.info.hash, changes = ?tx.result.execution.changes, "saving execution");

        // Log warning if a failed transaction has slot changes
        if !tx.result.execution.result.is_success() {
            let total_slot_changes: usize = changes.slots.len();

            if total_slot_changes > 0 {
                tracing::warn!(?tx, "Failed transaction contains {} slot change(s)", total_slot_changes);
            }
        }

        timed(|| self.temp.save_pending_execution(tx))
            .with(|m| {
                metrics::inc_storage_save_execution(m.elapsed, label::TEMP, m.result.is_ok());
                match &m.result {
                    Err(StorageError::EvmInputMismatch { .. }) => {
                        tracing::warn!("failed to save execution due to mismatch, will retry");
                    }
                    Err(e) => tracing::error!(reason = ?e, "failed to save execution"),
                    _ => (),
                }
            })
            .inspect(|_| self.cache.cache_account_and_slots_from_changes(changes))
    }

    /// Retrieves pending transactions being mined.
    pub fn pending_transactions(&self) -> Vec<TransactionExecution> {
        self.temp.read_pending_executions()
    }

    pub fn finish_pending_block(&self) -> Result<(PendingBlock, ExecutionChanges), StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::finish_pending_block", block_number = tracing::field::Empty).entered();
        tracing::debug!(storage = %label::TEMP, "finishing pending block");

        let result = timed(|| self.temp.finish_pending_block()).with(|m| {
            metrics::inc_storage_finish_pending_block(m.elapsed, label::TEMP, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to finish pending block");
            }
        });

        if let Ok((ref block, _)) = result {
            Span::with(|s| s.rec_str("block_number", &block.header.number));
        }

        result
    }

    pub fn save_genesis_block(&self, block: Block, accounts: Vec<Account>, changes: ExecutionChanges) -> Result<(), StorageError> {
        let block_number = block.number();

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::save_genesis_block", block_number = %block_number).entered();
        tracing::debug!(storage = %label::PERM, "saving genesis block");
        let tens_of_millions_gas_used = block.header.gas_used.as_u64() / 10_000_000;

        timed(|| self.perm.save_genesis_block(block, accounts, changes)).with(|m| {
            metrics::inc_storage_save_block(m.elapsed, label::PERM, tens_of_millions_gas_used, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to save genesis block");
            }
        })
    }

    pub fn save_block(&self, block: Block, changes: ExecutionChanges) -> Result<(), StorageError> {
        let block_number = block.number();

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::save_block", block_number = %block.number()).entered();
        tracing::debug!(storage = %label::PERM, block_number = %block_number, transactions_len = %block.transactions.len(), ?changes, "saving block");

        // check mined number
        let mined_number = self.read_mined_block_number();
        if not(block_number.is_zero()) && block_number != mined_number.next_block_number() {
            tracing::error!(%block_number, %mined_number, "failed to save block because mismatch with mined block number");
            return Err(StorageError::MinedNumberConflict {
                new: block_number,
                mined: mined_number,
            });
        }

        // check pending number
        let pending_header = self.read_pending_block_header();
        if block_number >= pending_header.0.number {
            tracing::error!(%block_number, pending_number = %pending_header.0.number, "failed to save block because mismatch with pending block number");
            return Err(StorageError::PendingNumberConflict {
                new: block_number,
                pending: pending_header.0.number,
            });
        }

        // check mined block
        let existing_block = self.read_block(BlockFilter::Number(block_number))?;
        if existing_block.is_some() {
            tracing::error!(%block_number, %mined_number, "failed to save block because block with the same number already exists in the permanent storage");
            return Err(StorageError::BlockConflict { number: block_number });
        }

        let tens_of_millions_gas_used = block.header.gas_used.as_u64() / 10_000_000;

        timed(|| {
            let guard = self.transient_state_lock.write();
            self.perm.save_block(block, changes.clone())?;
            self.cache.cache_account_and_slots_latest_from_changes(changes);
            drop(guard);
            Ok(())
        })
        .with(|m| {
            metrics::inc_storage_save_block(m.elapsed, label::PERM, tens_of_millions_gas_used, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, %block_number, "failed to save block");
            }
        })?;

        self.set_mined_block_number(block_number);

        Ok(())
    }

    pub fn read_block(&self, filter: BlockFilter) -> Result<Option<Block>, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_block", %filter).entered();
        tracing::debug!(storage = %label::PERM, ?filter, "reading block");

        timed(|| self.perm.read_block(filter)).with(|m| {
            metrics::inc_storage_read_block(m.elapsed, label::PERM, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to read block");
            }
        })
    }

    pub fn read_block_with_changes(&self, filter: BlockFilter) -> Result<Option<(BlockRocksdb, BlockChangesRocksdb)>, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_block_with_changes", %filter).entered();
        tracing::debug!(storage = %label::PERM, ?filter, "reading block with changes");

        timed(|| self.perm.read_block_with_changes(filter)).with(|m| {
            metrics::inc_storage_read_block_with_changes(m.elapsed, label::PERM, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to read block with changes");
            }
        })
    }

    pub fn read_transaction(&self, tx_hash: Hash) -> Result<Option<TransactionStage>, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_transaction", %tx_hash).entered();

        // read from temp
        tracing::debug!(storage = %label::TEMP, %tx_hash, "reading transaction");
        let temp_tx = timed(|| self.temp.read_pending_execution(tx_hash)).with(|m| {
            metrics::inc_storage_read_transaction(m.elapsed, label::TEMP, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to read transaction from temporary storage");
            }
        })?;
        if let Some(tx_temp) = temp_tx {
            return Ok(Some(TransactionStage::Pending(tx_temp)));
        }

        // read from perm
        tracing::debug!(storage = %label::PERM, %tx_hash, "reading transaction");
        let perm_tx = timed(|| self.perm.read_transaction(tx_hash)).with(|m| {
            metrics::inc_storage_read_transaction(m.elapsed, label::PERM, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to read transaction from permanent storage");
            }
        })?;
        Ok(perm_tx.map(TransactionStage::Mined))
    }

    pub fn read_logs(&self, filter: &LogFilter) -> Result<Vec<LogMessage>, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_logs", ?filter).entered();
        tracing::debug!(storage = %label::PERM, ?filter, "reading logs");

        timed(|| self.perm.read_logs(filter)).with(|m| {
            metrics::inc_storage_read_logs(m.elapsed, label::PERM, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to read logs");
            }
        })
    }

    // -------------------------------------------------------------------------
    // Direct state manipulation (for testing)
    // -------------------------------------------------------------------------

    #[cfg(feature = "dev")]
    pub fn set_storage_at(&self, address: Address, index: SlotIndex, value: SlotValue) -> Result<(), StorageError> {
        // Create a slot with the given index and value
        let slot = Slot::new(index, value);
        self.cache.clear();

        // Update permanent storage
        self.perm.save_slot(address, slot)?;

        // Update temporary storage
        self.temp.save_slot(address, slot)?;

        Ok(())
    }

    #[cfg(feature = "dev")]
    pub fn set_nonce(&self, address: Address, nonce: Nonce) -> Result<(), StorageError> {
        self.cache.clear();

        // Update permanent storage
        self.perm.save_account_nonce(address, nonce)?;

        // Update temporary storage
        self.temp.save_account_nonce(address, nonce)?;

        Ok(())
    }

    #[cfg(feature = "dev")]
    pub fn set_balance(&self, address: Address, balance: Wei) -> Result<(), StorageError> {
        self.cache.clear();

        // Update permanent storage
        self.perm.save_account_balance(address, balance)?;

        // Update temporary storage
        self.temp.save_account_balance(address, balance)?;

        Ok(())
    }

    #[cfg(feature = "dev")]
    pub fn set_code(&self, address: Address, code: Bytes) -> Result<(), StorageError> {
        self.cache.clear();
        // Update permanent storage
        self.perm.save_account_code(address, code.clone())?;

        // Update temporary storage
        self.temp.save_account_code(address, code)?;

        Ok(())
    }

    // -------------------------------------------------------------------------
    // General state
    // -------------------------------------------------------------------------

    #[cfg(feature = "dev")]
    /// Resets the storage to the genesis state.
    /// If a genesis.json file is available, it will be used.
    /// Otherwise, it will use the default genesis configuration.
    pub fn reset_to_genesis(&self) -> Result<(), StorageError> {
        tracing::info!("resetting storage to genesis state");

        self.cache.clear();

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::reset").entered();

        // reset perm
        tracing::debug!(storage = %label::PERM, "resetting permanent storage");
        timed(|| self.perm.reset()).with(|m| {
            metrics::inc_storage_reset(m.elapsed, label::PERM, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to reset permanent storage");
            }
        })?;

        // reset temp
        tracing::debug!(storage = %label::TEMP, "reseting temporary storage");
        timed(|| self.temp.reset()).with(|m| {
            metrics::inc_storage_reset(m.elapsed, label::TEMP, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to reset temporary storage");
            }
        })?;

        // Try to load genesis block from the genesis file or use default
        let genesis_block = if let Some(genesis_path) = &self.perm_config.genesis_file.genesis_path {
            if std::path::Path::new(genesis_path).exists() {
                match GenesisConfig::load_from_file(genesis_path) {
                    Ok(genesis_config) => match genesis_config.to_genesis_block() {
                        Ok(block) => {
                            tracing::info!("using genesis block from file: {:?}", genesis_path);
                            block
                        }
                        Err(e) => {
                            tracing::error!("failed to create genesis block from file: {:?}", e);
                            Block::genesis()
                        }
                    },
                    Err(e) => {
                        tracing::error!("failed to load genesis file: {:?}", e);
                        Block::genesis()
                    }
                }
            } else {
                tracing::error!("genesis file not found at: {:?}", genesis_path);
                Block::genesis()
            }
        } else {
            tracing::info!("using default genesis block");
            Block::genesis()
        };
        // Try to load genesis.json from the path specified in GenesisFileConfig
        // or use default genesis configuration
        let (genesis_accounts, genesis_slots) = if let Some(genesis_path) = &self.perm_config.genesis_file.genesis_path {
            if std::path::Path::new(genesis_path).exists() {
                tracing::info!("found genesis file at: {:?}", genesis_path);
                match GenesisConfig::load_from_file(genesis_path) {
                    Ok(genesis) => match genesis.to_stratus_accounts_and_slots() {
                        Ok((accounts, slots)) => {
                            tracing::info!("loaded {} accounts from genesis.json", accounts.len());
                            if !slots.is_empty() {
                                tracing::info!("loaded {} storage slots from genesis.json", slots.len());
                            }
                            (accounts, slots)
                        }
                        Err(e) => {
                            tracing::error!("failed to convert genesis accounts: {:?}", e);
                            // Fallback to test accounts
                            (test_accounts(), vec![])
                        }
                    },
                    Err(e) => {
                        tracing::error!("failed to load genesis file: {:?}", e);
                        // Fallback to test accounts
                        (test_accounts(), vec![])
                    }
                }
            } else {
                tracing::error!("genesis file not found at: {:?}", genesis_path);
                // Fallback to test accounts
                (test_accounts(), vec![])
            }
        } else {
            // No genesis path specified, use default genesis configuration
            match GenesisConfig::default().to_stratus_accounts_and_slots() {
                Ok((accounts, slots)) => {
                    tracing::info!("using default genesis configuration with {} accounts", accounts.len());
                    (accounts, slots)
                }
                Err(e) => {
                    tracing::error!("failed to convert default genesis accounts: {:?}", e);
                    // Fallback to test accounts
                    (test_accounts(), vec![])
                }
            }
        };
        // Save the genesis block
        self.save_block(genesis_block, ExecutionChanges::default())?;

        // accounts
        self.save_accounts(genesis_accounts)?;

        // Save slots if any
        if !genesis_slots.is_empty() {
            tracing::info!("saving {} storage slots from genesis", genesis_slots.len());
            for (address, slot) in genesis_slots {
                self.perm.save_slot(address, slot)?;
            }
        }

        // block number
        self.set_mined_block_number(BlockNumber::ZERO);

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Utils
    // -------------------------------------------------------------------------

    /// Translates a block filter to a specific storage point-in-time indicator.
    pub fn translate_to_point_in_time(&self, block_filter: BlockFilter) -> Result<PointInTime, StorageError> {
        match block_filter {
            BlockFilter::Pending => Ok(PointInTime::Pending),
            BlockFilter::Latest => Ok(PointInTime::Mined),
            BlockFilter::Earliest => Ok(PointInTime::MinedPast(BlockNumber::ZERO)),
            BlockFilter::Number(number) => Ok(PointInTime::MinedPast(number)),
            BlockFilter::Hash(_) | BlockFilter::Timestamp(_) => match self.read_block(block_filter)? {
                Some(block) => Ok(PointInTime::MinedPast(block.header.number)),
                None => Err(StorageError::BlockNotFound { filter: block_filter }),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eth::executor::EvmExecutionResult;
    use crate::eth::executor::EvmInput;
    use crate::eth::primitives::ExecutionAccountChanges;
    use crate::eth::primitives::ExecutionInfo;
    use crate::eth::primitives::ExecutionResult;
    use crate::eth::primitives::Signature;
    use crate::eth::primitives::SlotValue;
    use crate::eth::primitives::TransactionInfo;
    use crate::eth::primitives::TransactionInput;
    use crate::eth::primitives::Wei;

    /// Mines a block applying `changes`
    fn mine_block(storage: &StratusStorage, changes: ExecutionChanges) -> BlockNumber {
        let (header, _) = storage.read_pending_block_header();
        let evm_input = EvmInput::from_eth_transaction(&TransactionInput::default(), header.number, *header.timestamp);

        let mut result = EvmExecutionResult::default();
        result.execution.result = ExecutionResult::Success;
        result.execution.changes = changes;

        let tx = TransactionExecution::new(TransactionInfo::default(), Signature::default(), ExecutionInfo::default(), evm_input, result);
        storage.save_execution(tx).expect("save execution");

        let (block, block_changes) = storage.finish_pending_block().expect("finish pending block");
        storage.save_block(block.into(), block_changes).expect("save block");

        storage.read_mined_block_number()
    }

    /// An `eth_call` pinned to a block that is no longer the latest must read the historical
    /// state at its captured block, not the current latest state.
    #[test]
    fn read_slot_for_call_pinned_to_older_block_must_not_read_latest_state() {
        let storage = StratusStorage::new_test().expect("failed to build test storage");

        let address = Address::new([0xAA; 20]);
        let index = SlotIndex::ZERO;

        // Mine a block setting slot S = 100. The eth_call captures this block.
        let mut changes1 = ExecutionChanges::default();
        changes1.slots.insert((address, index), SlotValue::from([100u64, 0, 0, 0]));
        let call_block = mine_block(&storage, changes1);

        // A new block is mined while the call is in flight, changing the slot to 200.
        let mut changes2 = ExecutionChanges::default();
        changes2.slots.insert((address, index), SlotValue::from([200u64, 0, 0, 0]));
        let latest = mine_block(&storage, changes2);
        assert_ne!(call_block, latest);

        // The in-flight call (pinned to the first block) reads the slot.
        let slot = storage
            .read_slot(address, index, PointInTime::Mined, ReadKind::Call((call_block, TxCount::Full)))
            .expect("read slot");

        // Must reflect the first block (100), not the freshly mined latest (200).
        assert_eq!(slot.value, SlotValue::from([100u64, 0, 0, 0]));
    }

    #[test]
    fn read_account_for_call_pinned_to_older_block_must_not_read_latest_state() {
        let storage = StratusStorage::new_test().expect("failed to build test storage");

        let address = Address::new([0xBB; 20]);

        // Mine a block setting the account balance to 100. The eth_call captures this block.
        let mut changes1 = ExecutionChanges::default();
        changes1.accounts.insert(
            address,
            ExecutionAccountChanges::from_changed(Account::new_with_balance(address, Wei::from(100u64))),
        );
        let call_block = mine_block(&storage, changes1);

        // A new block is mined while the call is in flight, changing the balance to 200.
        let mut changes2 = ExecutionChanges::default();
        changes2.accounts.insert(
            address,
            ExecutionAccountChanges::from_changed(Account::new_with_balance(address, Wei::from(200u64))),
        );
        let latest = mine_block(&storage, changes2);
        assert_ne!(call_block, latest);

        let account = storage
            .read_account(address, PointInTime::Mined, ReadKind::Call((call_block, TxCount::Full)))
            .expect("read account");

        // Must reflect the first block (100), not the freshly mined latest (200).
        assert_eq!(account.balance, Wei::from(100u64));
    }
}
