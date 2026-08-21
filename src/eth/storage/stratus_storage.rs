use tracing::Span;

use crate::eth::executor::AccountOriginalsReader;
use crate::eth::executor::Changes;
use crate::eth::executor::Complete;
use crate::eth::executor::Full;
use crate::eth::executor::TransactionExecution;
#[cfg(feature = "dev")]
use crate::eth::genesis::GenesisConfig;
use crate::eth::rpc::BlockFilter;
use crate::eth::rpc::LogFilter;
use crate::eth::storage::ExecutionKind;
use crate::eth::storage::InMemoryTemporaryStorage;
use crate::eth::storage::RocksPermanentStorage;
use crate::eth::storage::StorageCache;
use crate::eth::storage::StorageError;
use crate::eth::storage::TxCount;
use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
use crate::eth::storage::permanent::rocks::types::BlockRocksdb;
use crate::eth::storage::resolve_pending;
use crate::eth::types::Account;
use crate::eth::types::Address;
use crate::eth::types::Block;
use crate::eth::types::BlockNumber;
#[cfg(feature = "dev")]
use crate::eth::types::Bytes;
use crate::eth::types::ExternalBlock;
use crate::eth::types::Hash;
use crate::eth::types::LogMessage;
#[cfg(feature = "dev")]
use crate::eth::types::Nonce;
use crate::eth::types::PendingBlock;
use crate::eth::types::PendingBlockHeader;
use crate::eth::types::PointInTime;
use crate::eth::types::Slot;
use crate::eth::types::SlotIndex;
#[cfg(feature = "dev")]
use crate::eth::types::SlotValue;
use crate::eth::types::TransactionStage;
use crate::eth::types::UnixTime;
#[cfg(feature = "dev")]
use crate::eth::types::Wei;
#[cfg(feature = "dev")]
use crate::eth::types::primitives::test_accounts;
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
    /// Reads the latest (mined tip) value from the cache, if present.
    fn read_latest_cache(s: &StratusStorage, key: Self::Key) -> Option<Self>;
    /// Reads from temporary (pending) storage.
    fn read_temp(s: &StratusStorage, key: Self::Key, kind: ExecutionKind) -> Option<Self>;
    /// Reads from permanent storage at the resolved mined point.
    fn read_perm(s: &StratusStorage, key: Self::Key, point: MinedPointInTime<'_>) -> Result<Self, StorageError>;
    /// Caches the value as a latest (mined tip) entry, if not already cached.
    fn cache_latest_if_missing(s: &StratusStorage, key: Self::Key, value: Self);
}

impl EntityRead for Account {
    type Key = Address;

    fn read_temp(s: &StratusStorage, address: Address, kind: ExecutionKind) -> Option<Self> {
        tracing::debug!(storage = %label::TEMP, %address, "reading account");
        timed(|| s.temp.read_account(address, kind)).with(|m| {
            if m.result.is_some() {
                metrics::inc_storage_read_account(m.elapsed, label::TEMP, PointInTime::Pending, true);
            }
        })
    }

    fn read_latest_cache(s: &StratusStorage, address: Address) -> Option<Self> {
        timed(|| s.cache.get_account_latest(address)).with(|m| {
            if m.result.is_some() {
                tracing::debug!(storage = %label::CACHE, %address, "account found in cache");
                metrics::inc_storage_read_account(m.elapsed, label::CACHE, PointInTime::Latest, true);
            }
        })
    }

    fn read_perm(s: &StratusStorage, address: Address, point: MinedPointInTime<'_>) -> Result<Self, StorageError> {
        tracing::debug!(storage = %label::PERM, %address, "reading account");
        let account = timed(|| s.perm.read_account(address, &point)).with(|m| {
            m.result
                .as_ref()
                .inspect(|opt| {
                    metrics::inc_storage_read_account(m.elapsed, label::PERM, point, opt.is_some());
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

    fn cache_latest_if_missing(s: &StratusStorage, address: Address, account: Self) {
        s.cache.cache_account_latest_if_missing(address, account);
    }
}

impl EntityRead for Slot {
    type Key = (Address, SlotIndex);

    fn read_temp(s: &StratusStorage, key: (Address, SlotIndex), kind: ExecutionKind) -> Option<Self> {
        let (address, index) = key;
        tracing::debug!(storage = %label::TEMP, %address, %index, "reading slot");
        timed(|| s.temp.read_slot(address, index, kind)).with(|m| {
            if m.result.is_some() {
                metrics::inc_storage_read_slot(m.elapsed, label::TEMP, PointInTime::Pending, true);
            }
        })
    }

    fn read_latest_cache(s: &StratusStorage, key: (Address, SlotIndex)) -> Option<Self> {
        let (address, index) = key;
        timed(|| s.cache.get_slot_latest(address, index)).with(|m| {
            if m.result.is_some() {
                tracing::debug!(storage = %label::CACHE, %address, slot = ?m.result, "slot found in cache");
                metrics::inc_storage_read_slot(m.elapsed, label::CACHE, PointInTime::Latest, true);
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
                    metrics::inc_storage_read_slot(m.elapsed, label::PERM, point, opt.is_some());
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

    fn cache_latest_if_missing(s: &StratusStorage, key: (Address, SlotIndex), slot: Self) {
        let (address, _) = key;
        s.cache.cache_slot_latest_if_missing(address, slot);
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

        use crate::eth::storage::cache::CacheConfig;

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
        self.temp.set_pending_header(block.number(), block.timestamp());
    }

    pub fn set_pending_header(&self, number: BlockNumber, timestamp: UnixTime) {
        self.temp.set_pending_header(number, timestamp);
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
    fn read<E: resolve_pending::Resolve>(&self, key: E::Key, kind: ExecutionKind) -> Result<E, StorageError> {
        let (value, found_at) = 'query: {
            match E::resolve(self, key, kind) {
                resolve_pending::Resolved::Temp(value) => break 'query (value, FoundAt::Temp),
                resolve_pending::Resolved::Miss(mined_point) => {
                    let found_at = match &mined_point {
                        MinedPointInTime::Latest(_, _) =>
                        // Latest: try latest cache while guard is held, then fall through to perm.
                        {
                            if let Some(value) = E::read_latest_cache(self, key) {
                                break 'query (value, FoundAt::Cache);
                            }
                            // If it wasnt found in the cache and we still have the guard the value can only be read in perm latest
                            FoundAt::PermLatest
                        }
                        MinedPointInTime::Past(_, _) => FoundAt::PermHistorical,
                    };
                    break 'query (E::read_perm(self, key, mined_point)?, found_at);
                }
            }
        };

        // Cache non-historical reads according to the point-in-time and where the value came from.
        match (kind, found_at) {
            (ExecutionKind::Transaction, _) => (),
            // A pending read that hit perm (i.e. not in any cache/temp) is already mined, so cache latest.
            // OR A mined read that hit perm is the latest state, so populate the latest cache.
            (_, FoundAt::PermLatest) => {
                E::cache_latest_if_missing(self, key, value.clone());
            }
            // Cache / Historical / (Mined, Temp): nothing to cache.
            _ => {}
        }
        Ok(value)
    }

    pub fn read_account(&self, address: Address, kind: ExecutionKind) -> Result<Account, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("storage::read_account", %address).entered();
        self.read::<Account>(address, kind)
    }

    pub fn read_slot(&self, address: Address, index: SlotIndex, kind: ExecutionKind) -> Result<Slot, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("storage::read_slot", %address, %index).entered();
        self.read::<Slot>((address, index), kind)
    }

    // -------------------------------------------------------------------------
    // Blocks
    // -------------------------------------------------------------------------

    pub fn save_execution(&self, tx: TransactionExecution) -> Result<(), StorageError> {
        let changes = tx.output.changes.clone();

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::save_execution", tx_hash = %tx.info.hash).entered();
        tracing::debug!(storage = %label::TEMP, tx_hash = %tx.info.hash, changes = ?tx.output.changes, "saving execution");

        // Log warning if a failed transaction has slot changes
        if !tx.output.result.is_success() {
            let total_slot_changes: usize = changes.slots.len();

            if total_slot_changes > 0 {
                tracing::warn!(?tx, "Failed transaction contains {} slot change(s)", total_slot_changes);
            }
        }

        timed(|| self.temp.save_pending_execution(tx)).with(|m| {
            metrics::inc_storage_save_execution(m.elapsed, label::TEMP, m.result.is_ok());
            match &m.result {
                Err(StorageError::EvmInputMismatch { .. }) => {
                    tracing::warn!("failed to save execution due to mismatch, will retry");
                }
                Err(e) => tracing::error!(reason = ?e, "failed to save execution"),
                _ => (),
            }
        })
    }

    /// Retrieves pending transactions being mined.
    pub fn pending_transactions(&self) -> Vec<TransactionExecution> {
        self.temp.read_pending_executions()
    }

    pub fn finish_pending_block(&self) -> (PendingBlock, Changes<Full>) {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::finish_pending_block", block_number = tracing::field::Empty).entered();
        tracing::debug!(storage = %label::TEMP, "finishing pending block");

        let result = timed(|| self.temp.finish_pending_block()).with(|m| {
            metrics::inc_storage_finish_pending_block(m.elapsed);
        });

        Span::with(|s| s.rec_str("block_number", &result.0.header.number));

        result
    }

    pub fn save_genesis_block(&self, block: Block, accounts: Vec<Account>, changes: Changes<Complete>) -> Result<(), StorageError> {
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

    pub fn save_block(&self, block: Block, changes: Changes<Full>) -> Result<(), StorageError> {
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
            self.cache.cache_account_and_slots_latest_from_changes(&changes);
            self.perm.save_block(block, changes.complete())?;
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
        self.save_block(genesis_block, Changes::default())?;

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
            BlockFilter::Latest => Ok(PointInTime::Latest),
            BlockFilter::Earliest => Ok(PointInTime::Past(BlockNumber::ZERO)),
            BlockFilter::Number(number) => Ok(PointInTime::Past(number)),
            BlockFilter::Hash(_) | BlockFilter::Timestamp(_) => match self.read_block(block_filter)? {
                Some(block) => Ok(PointInTime::Past(block.header.number)),
                None => Err(StorageError::BlockNotFound { filter: block_filter }),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eth::executor::AccountChanges;
    use crate::eth::executor::CompleteValue;
    use crate::eth::executor::ExecutionResult;
    use crate::eth::executor::TransactionExecutionInput;
    use crate::eth::executor::TransactionExecutionOutput;
    use crate::eth::types::Signature;
    use crate::eth::types::SlotValue;
    use crate::eth::types::TransactionInfo;
    use crate::eth::types::TransactionInput;
    use crate::eth::types::Wei;

    /// Mines a block applying `changes`
    fn mine_block(storage: &StratusStorage, changes: Changes<Full>) -> BlockNumber {
        let (header, _) = storage.read_pending_block_header();
        let evm_input = TransactionExecutionInput::from_eth_transaction(&TransactionInput::default(), header.number, *header.timestamp);

        let result = TransactionExecutionOutput {
            result: ExecutionResult::Success,
            changes,
            ..Default::default()
        };

        let tx = TransactionExecution::new(TransactionInfo::default(), Signature::default(), evm_input, result);
        storage.save_execution(tx).expect("save execution");

        let (block, block_changes) = storage.finish_pending_block();
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
        let mut changes1 = Changes::default();
        changes1
            .slots
            .insert((address, index), CompleteValue::Changed(SlotValue::from([100u64, 0, 0, 0])));
        let call_block = mine_block(&storage, changes1);

        // A new block is mined while the call is in flight, changing the slot to 200.
        let mut changes2 = Changes::default();
        changes2
            .slots
            .insert((address, index), CompleteValue::Changed(SlotValue::from([200u64, 0, 0, 0])));
        let latest = mine_block(&storage, changes2);
        assert_ne!(call_block, latest);

        // The in-flight call (pinned to the first block) reads the slot.
        let slot = storage.read_slot(address, index, ExecutionKind::CallLatest(call_block)).expect("read slot");

        // Must reflect the first block (100), not the freshly mined latest (200).
        assert_eq!(slot.value, SlotValue::from([100u64, 0, 0, 0]));
    }

    #[test]
    fn read_account_for_call_pinned_to_older_block_must_not_read_latest_state() {
        let storage = StratusStorage::new_test().expect("failed to build test storage");

        let address = Address::new([0xBB; 20]);

        // Mine a block setting the account balance to 100. The eth_call captures this block.
        let mut changes1 = Changes::default();
        changes1
            .accounts
            .insert(address, AccountChanges::from_changed(Account::new_with_balance(address, Wei::from(100u64))));
        let call_block = mine_block(&storage, changes1);

        // A new block is mined while the call is in flight, changing the balance to 200.
        let mut changes2 = Changes::default();
        changes2
            .accounts
            .insert(address, AccountChanges::from_changed(Account::new_with_balance(address, Wei::from(200u64))));
        let latest = mine_block(&storage, changes2);
        assert_ne!(call_block, latest);

        let account = storage.read_account(address, ExecutionKind::CallLatest(call_block)).expect("read account");

        // Must reflect the first block (100), not the freshly mined latest (200).
        assert_eq!(account.balance, Wei::from(100u64));
    }
}
