use tracing::Span;

#[cfg(feature = "dev")]
use crate::eth::genesis::GenesisConfig;
use crate::eth::primitives::Account;
use crate::eth::primitives::AccountOriginalsReader;
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
use crate::eth::primitives::UnixTime;
use crate::eth::primitives::UnixTimeNow;
#[cfg(feature = "dev")]
use crate::eth::primitives::Wei;
#[cfg(feature = "dev")]
use crate::eth::primitives::test_accounts;
use crate::eth::storage::BlockReference;
use crate::eth::storage::InMemoryTemporaryStorage;
use crate::eth::storage::PendingBlockGuard;
use crate::eth::storage::ReadKind;
use crate::eth::storage::RocksPermanentStorage;
use crate::eth::storage::StorageCache;
use crate::eth::storage::TxCount;
use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
use crate::eth::storage::permanent::rocks::types::BlockRocksdb;
use crate::eth::storage::resolve_pending;
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
    last_saved: parking_lot::Mutex<Option<BlockReference>>,
    // CONTRACT: Always acquire a lock when reading slots or accounts from latest (cache OR perm) and when saving a block
    pub(super) transient_state_lock: parking_lot::RwLock<()>,
    #[cfg(feature = "dev")]
    perm_config: crate::eth::storage::permanent::PermanentStorageConfig,
}

impl AccountOriginalsReader for StratusStorage {
    fn read_accounts(&self, addresses: Vec<Address>) -> anyhow::Result<Vec<(Address, Account)>> {
        Ok(self.perm.read_accounts(addresses)?)
    }
}

pub use resolve_pending::MinedPointInTime;

/// Where a completed read obtained its value. Drives the post-read caching decision.
#[derive(Debug)]
enum FoundAt {
    /// Hit in a cache (pending or latest). Already cached; nothing to write.
    Cache,
    /// Found in temporary (pending or latest) storage.
    Temp,
    /// Read from permanent storage at the latest mined point.
    PermLatest,
    /// Read from permanent storage at a historical block.
    PermHistorical,
}

/// Abstraction over address-keyed ([`Account`]) and slot-keyed ([`Slot`]) reads
pub(super) trait EntityRead: Sized + Clone {
    type Key: Copy;

    /// Reads the pending (current block) value from the cache, if present.
    fn read_pending_cache(s: &StratusStorage, key: Self::Key) -> Option<Self>;
    /// Reads the latest (mined tip) value from the cache, if present.
    fn read_latest_cache(s: &StratusStorage, key: Self::Key) -> Option<Self>;
    /// Reads from temporary (pending) storage.
    fn read_temp(s: &StratusStorage, key: Self::Key, kind: ReadKind) -> Result<Option<Self>, StorageError>;
    /// Reads from permanent storage at the resolved mined point.
    fn read_perm(s: &StratusStorage, key: Self::Key, point: MinedPointInTime<'_>) -> Result<Self, StorageError>;
    /// Caches the value as a pending entry, if not already cached.
    fn cache_if_missing(s: &StratusStorage, key: Self::Key, value: Self);
    /// Caches the value as a latest (mined tip) entry, if not already cached.
    fn cache_latest_if_missing(s: &StratusStorage, key: Self::Key, value: Self);
}

impl EntityRead for Account {
    type Key = Address;

    fn read_pending_cache(s: &StratusStorage, address: Address) -> Option<Self> {
        timed(|| s.cache.get_account(address)).with(|m| {
            if m.result.is_some() {
                tracing::debug!(storage = %label::CACHE, %address, "account found in cache");
                metrics::inc_storage_read_account(m.elapsed, label::CACHE, PointInTime::Pending);
            }
        })
    }

    fn read_temp(s: &StratusStorage, address: Address, kind: ReadKind) -> Result<Option<Self>, StorageError> {
        tracing::debug!(storage = %label::TEMP, %address, "reading account");
        timed(|| s.temp.read_account(address, kind)).with(|m| {
            if m.result.as_ref().is_ok_and(|opt| opt.is_some()) {
                metrics::inc_storage_read_account(m.elapsed, label::TEMP, PointInTime::Pending);
            }
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to read account from temporary storage");
            }
        })
    }

    fn read_latest_cache(s: &StratusStorage, address: Address) -> Option<Self> {
        timed(|| s.cache.get_account_latest(address)).with(|m| {
            if m.result.is_some() {
                tracing::debug!(storage = %label::CACHE, %address, "account found in cache");
                metrics::inc_storage_read_account(m.elapsed, label::CACHE, PointInTime::Mined);
            }
        })
    }

    fn read_perm(s: &StratusStorage, address: Address, point: MinedPointInTime<'_>) -> Result<Self, StorageError> {
        tracing::debug!(storage = %label::PERM, %address, "reading account");
        let account = timed(|| s.perm.read_account(address, &point)).with(|m| {
            m.result
                .as_ref()
                .inspect(|opt| {
                    opt.is_some().then(|| metrics::inc_storage_read_account(m.elapsed, label::PERM, point));
                })
                .inspect_err(|err| tracing::error!(reason = ?err, "failed to read account from permanent storage"))
                .ok();
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

    fn cache_if_missing(s: &StratusStorage, _address: Address, account: Self) {
        s.cache.cache_account_if_missing(account);
    }

    fn cache_latest_if_missing(s: &StratusStorage, address: Address, account: Self) {
        s.cache.cache_account_latest_if_missing(address, account);
    }
}

impl EntityRead for Slot {
    type Key = (Address, SlotIndex);

    fn read_pending_cache(s: &StratusStorage, key: (Address, SlotIndex)) -> Option<Self> {
        let (address, index) = key;
        timed(|| s.cache.get_slot(address, index)).with(|m| {
            if m.result.is_some() {
                tracing::debug!(storage = %label::CACHE, %address, slot = ?m.result, "slot found in cache");
                metrics::inc_storage_read_slot(m.elapsed, label::CACHE, PointInTime::Pending);
            }
        })
    }

    fn read_temp(s: &StratusStorage, key: (Address, SlotIndex), kind: ReadKind) -> Result<Option<Self>, StorageError> {
        let (address, index) = key;
        tracing::debug!(storage = %label::TEMP, %address, %index, "reading slot");
        timed(|| s.temp.read_slot(address, index, kind)).with(|m| {
            if m.result.as_ref().is_ok_and(|opt| opt.is_some()) {
                metrics::inc_storage_read_slot(m.elapsed, label::TEMP, PointInTime::Pending);
            }
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to read slot from temporary storage");
            }
        })
    }

    fn read_latest_cache(s: &StratusStorage, key: (Address, SlotIndex)) -> Option<Self> {
        let (address, index) = key;
        timed(|| s.cache.get_slot_latest(address, index)).with(|m| {
            if m.result.is_some() {
                tracing::debug!(storage = %label::CACHE, %address, slot = ?m.result, "slot found in cache");
                metrics::inc_storage_read_slot(m.elapsed, label::CACHE, PointInTime::Mined);
            }
        })
    }

    fn read_perm(s: &StratusStorage, key: (Address, SlotIndex), point: MinedPointInTime<'_>) -> Result<Self, StorageError> {
        let (address, index) = key;
        tracing::debug!(storage = %label::PERM, %address, %index, %point, "reading slot");
        let slot = timed(|| s.perm.read_slot(address, index, &point)).with(|m| {
            m.result
                .as_ref()
                .inspect(|opt| {
                    opt.is_some().then(|| metrics::inc_storage_read_slot(m.elapsed, label::PERM, point));
                })
                .inspect_err(|err| tracing::error!(reason = ?err, "failed to read slot from permanent storage"))
                .ok();
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

    fn cache_if_missing(s: &StratusStorage, key: (Address, SlotIndex), slot: Self) {
        let (address, _) = key;
        s.cache.cache_slot_if_missing(address, slot);
    }

    fn cache_latest_if_missing(s: &StratusStorage, key: (Address, SlotIndex), slot: Self) {
        let (address, _) = key;
        s.cache.cache_slot_latest_if_missing(address, slot);
    }
}

impl StratusStorage {
    fn validate_saved_continuity(last_saved: Option<BlockReference>, block: &Block) -> Result<(), StorageError> {
        let block_number = block.number();
        let (expected_number, expected_parent_hash) = match last_saved {
            None => (BlockNumber::ZERO, Hash::ZERO),
            Some(parent) => (parent.number.next_block_number(), parent.hash),
        };

        if block_number != expected_number {
            return Err(StorageError::MinedNumberConflict {
                new: block_number,
                mined: expected_number.prev().unwrap_or_default(),
            });
        }

        if block.header.parent_hash != expected_parent_hash {
            return Err(StorageError::ParentHashConflict {
                number: block_number,
                local: expected_parent_hash,
                external: block.header.parent_hash,
            });
        }

        Ok(())
    }

    pub fn validate_next_saved_block(&self, block: &Block) -> Result<(), StorageError> {
        Self::validate_saved_continuity(*self.last_saved.lock(), block)
    }

    /// Creates a new storage with the specified temporary and permanent implementations.
    pub fn new(
        temp: InMemoryTemporaryStorage,
        perm: RocksPermanentStorage,
        cache: StorageCache,
        #[cfg(feature = "dev")] perm_config: crate::eth::storage::permanent::PermanentStorageConfig,
    ) -> Result<Self, StorageError> {
        let last_saved = perm.read_chain_tip()?;

        let this = Self {
            temp,
            cache,
            perm,
            last_saved: parking_lot::Mutex::new(last_saved),
            transient_state_lock: parking_lot::RwLock::new(()),
            #[cfg(feature = "dev")]
            perm_config,
        };

        // create genesis block and accounts if necessary
        #[cfg(feature = "dev")]
        if !this.has_genesis()? {
            this.reset_to_genesis_inner()?;
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

        use crate::eth::storage::cache::CacheConfig;

        let temp = InMemoryTemporaryStorage::new(BlockReference::genesis());

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
            block_hash_cache_capacity: super::cache::DEFAULT_BLOCK_HASH_CACHE_CAPACITY,
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

    /// Prepares the guarded pending state to receive an external block.
    pub fn set_pending_from_external(&self, guard: &PendingBlockGuard<'_>, block: &ExternalBlock) {
        self.set_pending_header(guard, block.number(), block.timestamp());
    }

    pub fn pending_block_guard(&self) -> PendingBlockGuard<'_> {
        self.temp.pending_block_guard()
    }

    pub fn set_pending_header(&self, _guard: &PendingBlockGuard<'_>, number: BlockNumber, timestamp: UnixTime) {
        self.temp.set_pending_header(number, timestamp);
    }

    pub fn read_pending_parent_hash(&self, _guard: &PendingBlockGuard<'_>) -> Hash {
        self.temp.read_latest_sealed().hash
    }

    /// Publishes the identity of a block that was just sealed.
    ///
    /// This must happen at seal time rather than at save time: mining and saving can run in separate
    /// threads, so the next block may be sealed while this one is still on its way to the permanent
    /// storage, and it would find no parent to chain to.
    pub fn publish_block_hash(&self, number: BlockNumber, hash: Hash) {
        self.cache.cache_block_hash(number, hash);
    }

    /// Reads the hash of a mined block, falling back to the permanent storage on a cache miss.
    ///
    /// Misses are expected for blocks mined before this process started, since sealing a block is
    /// what publishes its hash.
    pub fn read_block_hash(&self, number: BlockNumber) -> Result<Option<Hash>, StorageError> {
        let last_saved = *self.last_saved.lock();
        let latest = self.temp.read_latest_sealed();
        if latest.number == number
            && match last_saved {
                None => true,
                Some(saved) => latest.number > saved.number,
            }
        {
            tracing::debug!(storage = %label::TEMP, %number, "unsaved block hash found in temporary storage");
            return Ok(Some(latest.hash));
        }

        if let Some(hash) = self.cache.get_block_hash(number) {
            tracing::debug!(storage = %label::CACHE, %number, "block hash found in cache");
            return Ok(Some(hash));
        }

        let Some(block) = self.read_block(BlockFilter::Number(number))? else {
            return Ok(None);
        };

        let hash = block.hash();
        self.cache.cache_block_hash_if_missing(number, hash);
        Ok(Some(hash))
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

    /// Generic read algorithm shared by [`read_account`] and [`read_slot`].
    fn read<E: resolve_pending::Resolve>(&self, key: E::Key, target_point_in_time: PointInTime, kind: ReadKind) -> Result<E, StorageError> {
        let (value, found_at) = 'query: {
            match E::resolve(self, target_point_in_time, key, kind)? {
                resolve_pending::Resolved::PendingCache(value) => break 'query (value, FoundAt::Cache),
                resolve_pending::Resolved::Temp(value) => break 'query (value, FoundAt::Temp),
                resolve_pending::Resolved::Miss(mined_point) => {
                    let found_at = match &mined_point {
                        MinedPointInTime::Latest(_, _) =>
                        // Latest: try latest cache while guard is held, then fall through to perm.
                        {
                            if let Some(value) = E::read_latest_cache(self, key) {
                                break 'query (value, FoundAt::Cache);
                            }
                            FoundAt::PermLatest
                        }
                        MinedPointInTime::Past(_, _) => FoundAt::PermHistorical,
                    };
                    break 'query (E::read_perm(self, key, mined_point)?, found_at);
                }
            }
        };

        // Cache non-historical reads according to the point-in-time and where the value came from.
        match (target_point_in_time, found_at) {
            // A pending read that hit perm (i.e. not in any cache/temp) is already mined, so cache in both.
            (PointInTime::Pending, FoundAt::PermLatest) => {
                E::cache_if_missing(self, key, value.clone());
                E::cache_latest_if_missing(self, key, value.clone());
            }
            // A pending read that hit temp was not found in the pending cache, so populate it.
            (PointInTime::Pending, FoundAt::Temp) => {
                E::cache_if_missing(self, key, value.clone());
            }
            // A mined read that hit perm is the latest state, so populate the latest cache.
            (PointInTime::Mined, FoundAt::PermLatest) => {
                E::cache_latest_if_missing(self, key, value.clone());
            }
            // Cache / Historical / (Mined, Temp): nothing to cache.
            _ => {}
        }
        Ok(value)
    }

    pub fn read_account(&self, address: Address, point_in_time: PointInTime, kind: ReadKind) -> Result<Account, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("storage::read_account", %address, %point_in_time).entered();
        self.read::<Account>(address, point_in_time, kind)
    }

    pub fn read_slot(&self, address: Address, index: SlotIndex, point_in_time: PointInTime, kind: ReadKind) -> Result<Slot, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("storage::read_slot", %address, %index, %point_in_time).entered();
        self.read::<Slot>((address, index), point_in_time, kind)
    }

    // -------------------------------------------------------------------------
    // Blocks
    // -------------------------------------------------------------------------

    pub fn save_execution(&self, _guard: &PendingBlockGuard<'_>, tx: TransactionExecution) -> Result<(), StorageError> {
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

    pub fn pending_block_to_seal(&self, _guard: &PendingBlockGuard<'_>) -> (PendingBlock, ExecutionChanges) {
        self.temp.pending_block_to_seal()
    }

    pub(crate) fn finish_pending_block(&self, _guard: &PendingBlockGuard<'_>, block: BlockReference, timestamp: UnixTimeNow) {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::finish_pending_block", block_number = %block.number).entered();
        tracing::debug!(storage = %label::TEMP, block_number = %block.number, "finishing pending block");

        timed(|| self.temp.finish_pending_block(block, timestamp)).with(|m| {
            metrics::inc_storage_finish_pending_block(m.elapsed, label::TEMP, true);
        });

        Span::with(|s| s.rec_str("block_number", &block.number));
    }

    pub fn save_genesis_block(&self, block: Block, accounts: Vec<Account>, changes: ExecutionChanges) -> Result<(), StorageError> {
        let block_number = block.number();
        let block_reference = BlockReference::from(&block);
        let mut last_saved = self.last_saved.lock();
        Self::validate_saved_continuity(*last_saved, &block)?;

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::save_genesis_block", block_number = %block_number).entered();
        tracing::debug!(storage = %label::PERM, "saving genesis block");
        let tens_of_millions_gas_used = block.header.gas_used.as_u64() / 10_000_000;

        timed(|| self.perm.save_genesis_block(block, accounts, changes)).with(|m| {
            metrics::inc_storage_save_block(m.elapsed, label::PERM, tens_of_millions_gas_used, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to save genesis block");
            }
        })?;

        *last_saved = Some(block_reference);
        self.set_mined_block_number(block_number);
        Ok(())
    }

    pub fn save_block(&self, block: Block, changes: ExecutionChanges) -> Result<(), StorageError> {
        let block_number = block.number();
        let block_reference = BlockReference::from(&block);
        let mut last_saved = self.last_saved.lock();

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::save_block", block_number = %block.number()).entered();
        tracing::debug!(storage = %label::PERM, block_number = %block_number, transactions_len = %block.transactions.len(), ?changes, "saving block");

        Self::validate_saved_continuity(*last_saved, &block)?;

        // check pending number
        let pending_header = self.read_pending_block_header();
        if block_number >= pending_header.0.number {
            tracing::error!(%block_number, pending_number = %pending_header.0.number, "failed to save block because mismatch with pending block number");
            return Err(StorageError::PendingNumberConflict {
                new: block_number,
                pending: pending_header.0.number,
            });
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

        *last_saved = Some(block_reference);
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
    pub(crate) fn reset_to_genesis(&self, _guard: &PendingBlockGuard<'_>) -> Result<(), StorageError> {
        self.reset_to_genesis_inner()
    }

    #[cfg(feature = "dev")]
    /// Resets the storage to the genesis state.
    /// If a genesis.json file is available, it will be used.
    /// Otherwise, it will use the default genesis configuration.
    fn reset_to_genesis_inner(&self) -> Result<(), StorageError> {
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
        *self.last_saved.lock() = None;

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
        let genesis_hash = genesis_block.hash();
        self.publish_block_hash(BlockNumber::ZERO, genesis_hash);
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
    use std::sync::Arc;

    use super::*;
    use crate::eth::executor::EvmExecutionResult;
    use crate::eth::executor::EvmInput;
    use crate::eth::miner::Miner;
    use crate::eth::miner::MinerMode;
    use crate::eth::primitives::ExecutionAccountChanges;
    use crate::eth::primitives::ExecutionInfo;
    use crate::eth::primitives::ExecutionResult;
    use crate::eth::primitives::Signature;
    use crate::eth::primitives::SlotValue;
    use crate::eth::primitives::TransactionInfo;
    use crate::eth::primitives::TransactionInput;
    use crate::eth::primitives::Wei;

    fn initialize_genesis(storage: &Arc<StratusStorage>) -> Block {
        if let Some(genesis) = storage.read_block(BlockFilter::Number(BlockNumber::ZERO)).unwrap() {
            return genesis;
        }

        let genesis = Block::genesis();
        storage
            .save_genesis_block(genesis.clone(), Vec::new(), ExecutionChanges::default())
            .expect("save genesis block");
        genesis
    }

    /// Mines a block applying `changes`
    fn mine_block(storage: &Arc<StratusStorage>, changes: ExecutionChanges) -> BlockNumber {
        initialize_genesis(storage);
        let miner = Miner::new(Arc::clone(storage), MinerMode::Automine);
        let (header, _) = storage.read_pending_block_header();
        let evm_input = EvmInput::from_eth_transaction(&TransactionInput::default(), header.number, *header.timestamp);

        let mut result = EvmExecutionResult::default();
        result.execution.result = ExecutionResult::Success;
        result.execution.changes = changes;

        let tx = TransactionExecution::new(TransactionInfo::default(), Signature::default(), ExecutionInfo::default(), evm_input, result);
        let session = miner.pending_session();
        session.append_execution(tx).expect("save execution");
        let (block, block_changes) = session.seal_local();
        storage.save_block(block, block_changes).expect("save block");

        storage.read_mined_block_number()
    }

    /// Mining and saving can run in separate threads, so a block must be chainable as soon as it is
    /// sealed, before it reaches the permanent storage.
    #[test]
    fn block_hash_is_readable_before_the_block_is_saved() {
        let storage = Arc::new(StratusStorage::new_test().expect("failed to build test storage"));

        let number = BlockNumber::from(7_u64);
        let hash = Hash::new([7; 32]);
        storage.publish_block_hash(number, hash);

        assert_eq!(storage.read_block_hash(number).expect("read block hash"), Some(hash));
        assert!(storage.read_block(BlockFilter::Number(number)).expect("read block").is_none());
    }

    #[test]
    fn latest_unsaved_hash_survives_cache_clear() {
        let storage = Arc::new(StratusStorage::new_test().expect("failed to build test storage"));
        let miner = Miner::new(Arc::clone(&storage), MinerMode::External);
        initialize_genesis(&storage);
        let (block, _) = miner.mine_local().expect("seal block");
        storage.clear_cache();

        assert_eq!(storage.read_block_hash(block.number()).expect("read unsaved hash"), Some(block.hash()));
        assert!(storage.read_block(BlockFilter::Number(block.number())).unwrap().is_none());
    }

    #[test]
    fn block_hash_falls_back_to_permanent_storage_when_the_cache_is_cold() {
        let storage = Arc::new(StratusStorage::new_test().expect("failed to build test storage"));

        let number = mine_block(&storage, ExecutionChanges::default());
        let hash = storage
            .read_block(BlockFilter::Number(number))
            .expect("read block")
            .expect("mined block should exist")
            .hash();

        // simulates a restart, where nothing was published by this process
        storage.cache.clear();

        assert_eq!(storage.read_block_hash(number).expect("read block hash"), Some(hash));
    }

    #[test]
    fn mined_blocks_are_chained_to_their_parent() {
        let storage = Arc::new(StratusStorage::new_test().expect("failed to build test storage"));

        let first = mine_block(&storage, ExecutionChanges::default());
        let second = mine_block(&storage, ExecutionChanges::default());
        assert_ne!(first, second);

        let read = |number| {
            storage
                .read_block(BlockFilter::Number(number))
                .expect("read block")
                .expect("block should exist")
        };

        assert_eq!(read(second).header.parent_hash, read(first).hash());
    }

    #[test]
    fn save_block_rejects_wrong_parent_without_advancing_last_saved() {
        let storage = Arc::new(StratusStorage::new_test().expect("failed to build test storage"));
        let miner = Miner::new(Arc::clone(&storage), MinerMode::Automine);
        let genesis = initialize_genesis(&storage);
        let (block, changes) = miner.mine_local().expect("seal block");

        let mut invalid = block.clone();
        invalid.header.parent_hash = Hash::ZERO;
        invalid.apply_default_hash();
        let error = storage.save_block(invalid, changes.clone()).expect_err("wrong parent should be rejected");
        assert!(matches!(
            error,
            StorageError::ParentHashConflict { number, local, external }
                if number == BlockNumber::ONE && local == genesis.hash() && external == Hash::ZERO
        ));

        storage.save_block(block, changes).expect("valid block should still save");
        assert_eq!(
            *storage.last_saved.lock(),
            Some(BlockReference {
                number: BlockNumber::ONE,
                hash: storage.read_block(BlockFilter::Number(BlockNumber::ONE)).unwrap().unwrap().hash(),
            })
        );
    }

    #[test]
    fn sealed_tip_can_run_ahead_of_saved_tip() {
        let storage = Arc::new(StratusStorage::new_test().expect("failed to build test storage"));
        let miner = Miner::new(Arc::clone(&storage), MinerMode::External);
        initialize_genesis(&storage);

        let (first, first_changes) = miner.mine_local().expect("seal first block");
        let (second, second_changes) = miner.mine_local().expect("seal second block");

        assert_eq!(second.header.parent_hash, first.hash());
        assert_eq!(storage.temp.read_latest_sealed(), BlockReference::from(&second));
        assert_eq!(
            *storage.last_saved.lock(),
            Some(BlockReference {
                number: BlockNumber::ZERO,
                hash: Block::genesis().hash(),
            })
        );

        storage.save_block(first, first_changes).expect("save first block");
        storage.save_block(second, second_changes).expect("save second block");
    }

    #[test]
    fn startup_preloads_legacy_saved_tip_for_next_parent() {
        use crate::eth::storage::cache::CacheConfig;

        let rocks_dir = tempfile::tempdir().expect("create rocks directory");
        let rocks_prefix = rocks_dir.path().join("preloaded-tip").to_string_lossy().into_owned();
        let perm = RocksPermanentStorage::new(
            Some(rocks_prefix.clone()),
            std::time::Duration::from_secs(240),
            super::super::permanent::RocksCfCacheConfig::default(),
            true,
            None,
            1024,
        )
        .expect("create permanent storage");

        let genesis = Block::genesis();
        perm.save_genesis_block(genesis.clone(), Vec::new(), ExecutionChanges::default())
            .expect("save genesis");
        let mut legacy = Block::new(BlockNumber::ONE, UnixTime::from(1_u64));
        legacy.header.parent_hash = genesis.hash();
        legacy.apply_hash(legacy.calculate_hash_v1());
        perm.save_block(legacy.clone(), ExecutionChanges::default()).expect("save legacy block");
        perm.set_mined_block_number(BlockNumber::ONE);

        let temp = InMemoryTemporaryStorage::new(BlockReference::from(&legacy));
        let cache = CacheConfig {
            slot_cache_capacity: 1,
            account_cache_capacity: 1,
            account_history_cache_capacity: 1,
            slot_history_cache_capacity: 1,
            block_hash_cache_capacity: 256,
        }
        .init();
        let storage = Arc::new(
            StratusStorage::new(
                temp,
                perm,
                cache,
                #[cfg(feature = "dev")]
                super::super::permanent::PermanentStorageConfig {
                    rocks_path_prefix: Some(rocks_prefix),
                    rocks_shutdown_timeout: std::time::Duration::from_secs(240),
                    rocks_cf_cache: super::super::permanent::RocksCfCacheConfig::default(),
                    rocks_disable_sync_write: false,
                    rocks_cf_size_metrics_interval: None,
                    genesis_file: crate::config::GenesisFileConfig::default(),
                    rocks_file_descriptors_limit: 1024,
                },
            )
            .expect("create storage"),
        );
        let miner = Miner::new(Arc::clone(&storage), MinerMode::Automine);

        let (block, _) = miner.mine_local().expect("seal next block");

        assert_eq!(block.number(), BlockNumber::from(2_u64));
        assert_eq!(block.header.parent_hash, legacy.hash());
    }

    /// An `eth_call` pinned to a block that is no longer the latest must read the historical
    /// state at its captured block, not the current latest state.
    #[test]
    fn read_slot_for_call_pinned_to_older_block_must_not_read_latest_state() {
        let storage = Arc::new(StratusStorage::new_test().expect("failed to build test storage"));

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
        let storage = Arc::new(StratusStorage::new_test().expect("failed to build test storage"));

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
