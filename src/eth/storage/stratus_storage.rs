use parking_lot::RwLockReadGuard;
use tracing::Span;

use super::InMemoryTemporaryStorage;
use super::RocksPermanentStorage;
use super::StorageCache;
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
        let mut missing_accounts = Vec::new();
        for account in accounts {
            let perm_account = self.perm.read_account(account.address, PointInTime::Mined)?;
            if perm_account.is_none() {
                missing_accounts.push(account);
            }
        }

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

    fn _read_account_perm(&self, address: Address, point_in_time: PointInTime) -> Result<Account, StorageError> {
        tracing::debug!(storage = %label::PERM, %address, "reading account");
        let account = timed(|| self.perm.read_account(address, point_in_time)).with(|m| {
            if m.result.as_ref().is_ok_and(|opt| opt.is_some()) {
                metrics::inc_storage_read_account(m.elapsed, label::PERM, point_in_time);
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

    // For calls, this function returns a guard if latest is safe to read
    fn _latest_is_valid(&self, point_in_time: PointInTime, kind: ReadKind) -> (bool, Option<RwLockReadGuard<'_, ()>>) {
        if matches!(point_in_time, PointInTime::MinedPast(_)) {
            return (false, None);
        }
        match kind {
            ReadKind::Call((block_number, _)) => {
                let guard = self.transient_state_lock.read();
                // Check if the provided block number is less than or equal to the mined block number
                let is_valid = block_number <= self.read_mined_block_number();
                (is_valid, (is_valid).then_some(guard))
            }
            _ => (true, None),
        }
    }

    pub fn read_account(&self, address: Address, mut point_in_time: PointInTime, kind: ReadKind) -> Result<Account, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("storage::read_account", %address, %point_in_time).entered();

        let (account, found_in_perm) = 'query: {
            if point_in_time == PointInTime::Pending {
                if matches!(kind, ReadKind::Transaction)
                    && let Some(account) = self._read_account_pending_cache(address)
                {
                    return Ok(account);
                }

                if let Some(account) = self._read_account_temp(address, kind)? {
                    tracing::debug!(storage = %label::TEMP, %address, ?account, "account found in temporary storage");
                    break 'query (account, false);
                }
            }

            let (is_valid, guard) = self._latest_is_valid(point_in_time, kind);
            if is_valid {
                if let Some(account) = self._read_account_latest_cache(address) {
                    return Ok(account);
                }
            } else if let ReadKind::Call((block_number, _)) = kind
                && !matches!(point_in_time, PointInTime::MinedPast(_))
            {
                point_in_time = PointInTime::MinedPast(block_number);
            }

            // always read from perm if necessary
            let ret = (self._read_account_perm(address, point_in_time)?, true);
            if let Some(inner) = guard {
                RwLockReadGuard::unlock_fair(inner);
            }
            ret
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

    fn _read_slot_perm(&self, address: Address, index: SlotIndex, point_in_time: PointInTime) -> Result<Slot, StorageError> {
        tracing::debug!(storage = %label::PERM, %address, %index, %point_in_time, "reading slot");
        let slot = timed(|| self.perm.read_slot(address, index, point_in_time)).with(|m| {
            if m.result.as_ref().is_ok_and(|opt| opt.is_some()) {
                metrics::inc_storage_read_slot(m.elapsed, label::PERM, point_in_time);
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
            if point_in_time == PointInTime::Pending {
                // tx can read from pending cache
                if matches!(kind, ReadKind::Transaction)
                    && let Some(slot) = self._read_slot_pending_cache(address, index)
                {
                    return Ok(slot);
                }

                if let Some(slot) = self._read_slot_temp(address, index, kind)? {
                    tracing::debug!(storage = %label::TEMP, %address, %index, value = %slot.value, "slot found in temporary storage");
                    break 'query (slot, false);
                }
            }

            let (is_valid, guard) = self._latest_is_valid(point_in_time, kind);
            if is_valid {
                if let Some(slot) = self._read_slot_latest_cache(address, index) {
                    return Ok(slot);
                }
            } else if let ReadKind::Call((block_number, _)) = kind
                && !matches!(point_in_time, PointInTime::MinedPast(_))
            {
                point_in_time = PointInTime::MinedPast(block_number);
            }

            // always read from perm if necessary
            let ret = (self._read_slot_perm(address, index, point_in_time)?, true);
            if let Some(inner) = guard {
                RwLockReadGuard::unlock_fair(inner);
            }
            ret
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

        timed(|| self.perm.save_genesis_block(block, accounts, changes)).with(|m| {
            metrics::inc_storage_save_block(m.elapsed, label::PERM, "genesis", "genesis", m.result.is_ok());
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

        // save block
        let (label_size_by_tx, label_size_by_gas) = (block.label_size_by_transactions(), block.label_size_by_gas());

        timed(|| {
            let guard = self.transient_state_lock.write();
            self.perm.save_block(block, changes.clone())?;
            self.cache.cache_account_and_slots_latest_from_changes(changes);
            drop(guard);
            Ok(())
        })
        .with(|m| {
            metrics::inc_storage_save_block(m.elapsed, label::PERM, label_size_by_tx, label_size_by_gas, m.result.is_ok());
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

    pub fn read_block_with_changes(&self, filter: BlockFilter) -> Result<Option<(Block, BlockChangesRocksdb)>, StorageError> {
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

    pub fn read_transaction(&self, tx_hash: Hash) -> Result<Option<TransactionExecution>, StorageError> {
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
            return Ok(Some(tx_temp));
        }

        // read from perm
        tracing::debug!(storage = %label::PERM, %tx_hash, "reading transaction");
        let perm_tx = timed(|| self.perm.read_transaction(tx_hash)).with(|m| {
            metrics::inc_storage_read_transaction(m.elapsed, label::PERM, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to read transaction from permanent storage");
            }
        })?;
        Ok(perm_tx)
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
            BlockFilter::Hash(_) => match self.read_block(block_filter)? {
                Some(block) => Ok(PointInTime::MinedPast(block.header.number)),
                None => Err(StorageError::BlockNotFound { filter: block_filter }),
            },
        }
    }
}
