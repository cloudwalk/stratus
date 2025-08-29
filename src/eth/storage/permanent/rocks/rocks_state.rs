use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Debug;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use rocksdb::DB;
use rocksdb::Direction;
use rocksdb::Options;
use rocksdb::WaitForCompactOptions;
use rocksdb::WriteBatch;
use rocksdb::WriteOptions;
use serde::Deserialize;
use serde::Serialize;
use sugars::btmap;

use super::cf_versions::CfAccountSlotsHistoryValue;
use super::cf_versions::CfAccountSlotsValue;
use super::cf_versions::CfAccountsHistoryValue;
use super::cf_versions::CfAccountsValue;
use super::cf_versions::CfBlocksByHashValue;
use super::cf_versions::CfBlocksByNumberValue;
use super::cf_versions::CfTransactionsValue;
use super::rocks_cf::RocksCfRef;
use super::rocks_config::CacheSetting;
use super::rocks_config::DbConfig;
use super::rocks_db::create_or_open_db;
use super::types::AccountRocksdb;
use super::types::AddressRocksdb;
use super::types::BlockNumberRocksdb;
use super::types::HashRocksdb;
use super::types::SlotIndexRocksdb;
use super::types::SlotValueRocksdb;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
#[cfg(feature = "dev")]
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
#[cfg(feature = "dev")]
use crate::eth::primitives::Nonce;
use crate::eth::primitives::PointInTime;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::TransactionMined;
#[cfg(feature = "dev")]
use crate::eth::primitives::Wei;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;
use crate::eth::storage::permanent::rocks::cf_versions::CfBlockChangesValue;
use crate::eth::storage::permanent::rocks::types::AccountChangesRocksdb;
use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
use crate::ext::OptionExt;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::log_and_err;
use crate::utils::GIGABYTE;

cfg_if::cfg_if! {
    if #[cfg(feature = "rocks_metrics")] {
        use parking_lot::Mutex;

        use rocksdb::statistics::Histogram;
        use rocksdb::statistics::Ticker;
        use std::collections::HashMap;


        use crate::infra::metrics::{Count, HistogramInt, Sum};
    }
}

pub fn generate_cf_options_map(cache_multiplier: Option<f32>) -> BTreeMap<&'static str, Options> {
    let cache_multiplier = cache_multiplier.unwrap_or(1.0);

    // multiplies the given size in GBs by the cache multiplier
    let cached_in_gigs_and_multiplied = |size_in_gbs: u64| -> CacheSetting {
        let size = (size_in_gbs as f32) * cache_multiplier;
        let size = GIGABYTE * size as usize;
        CacheSetting::Enabled(size)
    };

    btmap! {
        "accounts" => DbConfig::OptimizedPointLookUp.to_options(cached_in_gigs_and_multiplied(10)),
        "accounts_history" => DbConfig::HistoricalData.to_options(cached_in_gigs_and_multiplied(2)),
        "account_slots" => DbConfig::OptimizedPointLookUp.to_options(cached_in_gigs_and_multiplied(30)),
        "account_slots_history" => DbConfig::HistoricalData.to_options(cached_in_gigs_and_multiplied(2)),
        "transactions" => DbConfig::Default.to_options(cached_in_gigs_and_multiplied(2)),
        "blocks_by_number" => DbConfig::Default.to_options(cached_in_gigs_and_multiplied(2)),
        "blocks_by_hash" => DbConfig::Default.to_options(cached_in_gigs_and_multiplied(2)),
        "block_changes" => DbConfig::Default.to_options(cached_in_gigs_and_multiplied(2)),
    }
}

/// Helper for creating a `RocksCfRef`, aborting if it wasn't declared in our option presets.
fn new_cf_ref<'a, K, V>(db: &'a Arc<DB>, column_family: &str, cf_options_map: &BTreeMap<&str, Options>) -> Result<RocksCfRef<'a, K, V>>
where
    K: Serialize + for<'de> Deserialize<'de> + Debug + std::hash::Hash + Eq + SerializeDeserializeWithContext + bincode::Encode + bincode::Decode<()>,
    V: Serialize + for<'de> Deserialize<'de> + Debug + Clone + SerializeDeserializeWithContext + bincode::Encode + bincode::Decode<()>,
{
    tracing::debug!(column_family = column_family, "creating new column family");

    cf_options_map
        .get(column_family)
        .with_context(|| format!("matching column_family `{column_family}` given to `new_cf_ref` wasn't found in configuration map"))?;

    // NOTE: this doesn't create the CFs in the database, read `RocksCfRef` docs for details
    RocksCfRef::new(db, column_family)
}

/// State handler for our RocksDB storage, separating "tables" by column families.
///
/// With data separated by column families, writing and reading should be done via the `RocksCfRef` fields.
pub struct RocksStorageState {
    pub db: Arc<DB>,
    db_path: String,
    accounts: RocksCfRef<'static, AddressRocksdb, CfAccountsValue>,
    accounts_history: RocksCfRef<'static, (AddressRocksdb, BlockNumberRocksdb), CfAccountsHistoryValue>,
    account_slots: RocksCfRef<'static, (AddressRocksdb, SlotIndexRocksdb), CfAccountSlotsValue>,
    account_slots_history: RocksCfRef<'static, (AddressRocksdb, SlotIndexRocksdb, BlockNumberRocksdb), CfAccountSlotsHistoryValue>,
    pub transactions: RocksCfRef<'static, HashRocksdb, CfTransactionsValue>,
    pub blocks_by_number: RocksCfRef<'static, BlockNumberRocksdb, CfBlocksByNumberValue>,
    blocks_by_hash: RocksCfRef<'static, HashRocksdb, CfBlocksByHashValue>,
    block_changes: RocksCfRef<'static, BlockNumberRocksdb, CfBlockChangesValue>,
    /// Last collected stats for a histogram
    #[cfg(feature = "rocks_metrics")]
    prev_stats: Mutex<HashMap<HistogramInt, (Sum, Count)>>,
    /// Options passed at DB creation, stored for metrics
    ///
    /// a newly created `rocksdb::Options` object is unique, with an underlying pointer identifier inside of it, and is used to access
    /// the DB metrics, `Options` can be cloned, but two equal `Options` might not retrieve the same metrics
    #[cfg(feature = "rocks_metrics")]
    db_options: Options,
    shutdown_timeout: Duration,
    enable_sync_write: bool,
}

impl RocksStorageState {
    pub fn new(path: String, shutdown_timeout: Duration, cache_multiplier: Option<f32>, enable_sync_write: bool) -> Result<Self> {
        tracing::debug!("creating (or opening an existing) database with the specified column families");

        let cf_options_map = generate_cf_options_map(cache_multiplier);

        #[cfg_attr(not(feature = "rocks_metrics"), allow(unused_variables))]
        let (db, db_options) = create_or_open_db(&path, &cf_options_map).context("when trying to create (or open) rocksdb")?;

        if db.path().to_str().is_none() {
            bail!("db path doesn't isn't valid UTF-8: {:?}", db.path());
        }
        if db.path().file_name().is_none() {
            bail!("db path doesn't have a file name or isn't valid UTF-8: {:?}", db.path());
        }

        let state = Self {
            db_path: path,
            accounts: new_cf_ref(db, "accounts", &cf_options_map)?,
            accounts_history: new_cf_ref(db, "accounts_history", &cf_options_map)?,
            account_slots: new_cf_ref(db, "account_slots", &cf_options_map)?,
            account_slots_history: new_cf_ref(db, "account_slots_history", &cf_options_map)?,
            transactions: new_cf_ref(db, "transactions", &cf_options_map)?,
            blocks_by_number: new_cf_ref(db, "blocks_by_number", &cf_options_map)?,
            blocks_by_hash: new_cf_ref(db, "blocks_by_hash", &cf_options_map)?,
            block_changes: new_cf_ref(db, "block_changes", &cf_options_map)?,
            #[cfg(feature = "rocks_metrics")]
            prev_stats: Mutex::default(),
            #[cfg(feature = "rocks_metrics")]
            db_options,
            db: Arc::clone(db),
            shutdown_timeout,
            enable_sync_write,
        };

        tracing::debug!("opened database successfully");
        Ok(state)
    }

    #[cfg(test)]
    #[track_caller]
    pub fn new_in_testdir() -> anyhow::Result<(Self, tempfile::TempDir)> {
        let test_dir = tempfile::tempdir()?;
        let path = test_dir.as_ref().display().to_string();
        let state = Self::new(path, Duration::ZERO, None, true)?;
        Ok((state, test_dir))
    }

    /// Get the filename of the database path.
    #[cfg(feature = "metrics")]
    fn db_path_filename(&self) -> &str {
        self.db_path.rsplit('/').next().unwrap_or(&self.db_path)
    }

    pub fn preload_block_number(&self) -> Result<AtomicU32> {
        let block_number = self.blocks_by_number.last_key()?.unwrap_or_default().0;
        tracing::info!(%block_number, "preloaded block_number");
        Ok(AtomicU32::new(block_number))
    }

    #[cfg(feature = "dev")]
    pub fn reset(&self) -> Result<()> {
        self.accounts.clear()?;
        self.accounts_history.clear()?;
        self.account_slots.clear()?;
        self.account_slots_history.clear()?;
        self.transactions.clear()?;
        self.blocks_by_number.clear()?;
        self.blocks_by_hash.clear()?;
        Ok(())
    }

    /// Updates the in-memory state with changes from transaction execution
    fn prepare_batch_with_execution_changes(&self, changes: ExecutionChanges, block_number: BlockNumber, batch: &mut WriteBatch) -> Result<()> {
        let mut block_changes = BlockChangesRocksdb::default();
        let block_number = block_number.into();

        for (address, change) in changes.accounts {
            let address: AddressRocksdb = address.into();
            let mut account_change_entry = AccountChangesRocksdb::default();

            let mut account_info_entry = self.accounts.get(&address)?.unwrap_or(AccountRocksdb::default().into());

            if let Some(nonce) = change.nonce {
                account_info_entry.nonce = nonce.into();
                account_change_entry.nonce = Some(nonce.into());
            }
            if let Some(balance) = change.balance {
                account_info_entry.balance = balance.into();
                account_change_entry.balance = Some(balance.into());
            }
            if let Some(bytecode) = change.bytecode {
                account_info_entry.bytecode = bytecode.clone().map_into();
                account_change_entry.bytecode = bytecode.map_into();
            }

            self.accounts.prepare_batch_insertion([(address, account_info_entry.clone())], batch)?;
            self.accounts_history
                .prepare_batch_insertion([((address, block_number), account_info_entry.into_inner().into())], batch)?;

            account_change_entry
                .has_changes()
                .then(|| block_changes.account_changes.insert(address, account_change_entry));
        }

        for ((address, slot_index), slot_change) in changes.slots {
            let slot_index = slot_index.into();
            let slot_value: SlotValueRocksdb = slot.value.into();

            account_change_entry.slot_changes.insert(slot_index, slot_value);
            self.account_slots
                .prepare_batch_insertion([((address, slot_index), slot_value.into())], batch)?;
            self.account_slots_history
                .prepare_batch_insertion([((address, slot_index, block_number), slot_value.into())], batch)?;
        }

        self.block_changes.prepare_batch_insertion([(block_number, block_changes.into())], batch)?;

        Ok(())
    }

    pub fn read_transaction(&self, tx_hash: Hash) -> Result<Option<TransactionMined>> {
        let Some(block_number) = self.transactions.get(&tx_hash.into())? else {
            return Ok(None);
        };

        let Some(block) = self.blocks_by_number.get(&block_number)? else {
            return log_and_err!("the block that the transaction was supposed to be in was not found")
                .with_context(|| format!("block_number = {block_number:?} tx_hash = {tx_hash}"));
        };
        let block = block.into_inner();

        let transaction = block.transactions.into_iter().find(|tx| Hash::from(tx.input.hash) == tx_hash);

        match transaction {
            Some(tx) => {
                tracing::trace!(%tx_hash, "transaction found");
                Ok(Some(TransactionMined::from_rocks_primitives(tx, block_number.into_inner(), block.header.hash)))
            }
            None => log_and_err!("rocks error, transaction wasn't found in block where the index pointed at")
                .with_context(|| format!("block_number = {block_number:?} tx_hash = {tx_hash}")),
        }
    }

    pub fn read_logs(&self, filter: &LogFilter) -> Result<Vec<LogMined>> {
        let is_block_number_in_end_range = |number: BlockNumber| match filter.to_block.as_ref() {
            Some(&last_block) => number <= last_block,
            None => true,
        };

        let iter = self
            .blocks_by_number
            .iter_from(BlockNumberRocksdb::from(filter.from_block), Direction::Forward)?;

        let mut logs_result = vec![];

        for next in iter {
            let (number, block) = next?;

            if !is_block_number_in_end_range(number.into()) {
                break;
            }

            let block = block.into_inner();
            let logs = block.transactions.into_iter().enumerate().flat_map(|(tx_index, transaction)| {
                transaction.logs.into_iter().map(move |log| {
                    LogMined::from_rocks_primitives(log.log, block.header.number, block.header.hash, tx_index, transaction.input.hash, log.index)
                })
            });

            let filtered_logs = logs.filter(|log| filter.matches(log));
            logs_result.extend(filtered_logs);
        }
        Ok(logs_result)
    }

    pub fn read_slot(&self, address: Address, index: SlotIndex, point_in_time: PointInTime) -> Result<Option<Slot>> {
        if address.is_coinbase() {
            return Ok(None);
        }

        match point_in_time {
            PointInTime::Mined | PointInTime::Pending => {
                let query_params = (address.into(), index.into());

                let Some(account_slot_value) = self.account_slots.get(&query_params)? else {
                    return Ok(None);
                };

                Ok(Some(Slot {
                    index,
                    value: account_slot_value.into_inner().into(),
                }))
            }
            PointInTime::MinedPast(block_number) => {
                tracing::debug!(?address, ?index, ?block_number, "searching slot");

                let key = (address.into(), (index).into(), block_number.into());

                if let Some(((rocks_address, rocks_index, block), value)) = self.account_slots_history.seek(key)?
                    && rocks_index == index.into()
                    && rocks_address == address.into()
                {
                    tracing::debug!(?block, ?rocks_index, ?rocks_address, "slot found in rocksdb storage");
                    return Ok(Some(Slot {
                        index: rocks_index.into(),
                        value: value.into_inner().into(),
                    }));
                }
                Ok(None)
            }
        }
    }

    pub fn read_account(&self, address: Address, point_in_time: PointInTime) -> Result<Option<Account>> {
        if address.is_coinbase() || address.is_zero() {
            return Ok(None);
        }

        match point_in_time {
            PointInTime::Mined | PointInTime::Pending => {
                let Some(inner_account) = self.accounts.get(&address.into())? else {
                    tracing::trace!(%address, "account not found");
                    return Ok(None);
                };

                let account = inner_account.to_account(address);
                tracing::trace!(%address, ?account, "account found");
                Ok(Some(account))
            }
            PointInTime::MinedPast(block_number) => {
                tracing::debug!(?address, ?block_number, "searching account");

                let key = (address.into(), block_number.into());

                if let Some(((addr, block), account_info)) = self.accounts_history.seek(key)?
                    && addr == address.into()
                {
                    tracing::debug!(?block, ?address, "account found in rocksdb storage");
                    return Ok(Some(account_info.to_account(address)));
                }
                Ok(None)
            }
        }
    }

    pub fn read_block(&self, selection: BlockFilter) -> Result<Option<Block>> {
        tracing::debug!(?selection, "reading block");

        let block = match selection {
            BlockFilter::Latest | BlockFilter::Pending => self.blocks_by_number.last_value(),
            BlockFilter::Earliest => self.blocks_by_number.first_value(),
            BlockFilter::Number(block_number) => self.blocks_by_number.get(&block_number.into()),
            BlockFilter::Hash(block_hash) =>
                if let Some(block_number) = self.blocks_by_hash.get(&block_hash.into())? {
                    self.blocks_by_number.get(&block_number)
                } else {
                    Ok(None)
                },
        };
        block.map(|block_option| block_option.map(|block| block.into_inner().into()))
    }

    pub fn save_accounts(&self, accounts: Vec<Account>) -> Result<()> {
        let mut write_batch = WriteBatch::default();

        tracing::debug!(?accounts, "preparing accounts cf batch");
        self.accounts.prepare_batch_insertion(
            accounts.iter().cloned().map(|acc| {
                let tup = <(AddressRocksdb, AccountRocksdb)>::from(acc);
                (tup.0, tup.1.into())
            }),
            &mut write_batch,
        )?;

        tracing::debug!(?accounts, "preparing accounts history batch");
        self.accounts_history.prepare_batch_insertion(
            accounts.iter().cloned().map(|acc| {
                let tup = <(AddressRocksdb, AccountRocksdb)>::from(acc);
                ((tup.0, 0u32.into()), tup.1.into())
            }),
            &mut write_batch,
        )?;

        self.write_in_batch_for_multiple_cfs(write_batch)
    }

    pub fn save_genesis_block(&self, block: Block, accounts: Vec<Account>, account_changes: ExecutionChanges) -> Result<()> {
        let mut batch = WriteBatch::default();

        let mut txs_batch = vec![];
        for transaction in block.transactions.iter().cloned() {
            txs_batch.push((transaction.input.hash.into(), transaction.block_number.into()));
        }
        self.transactions.prepare_batch_insertion(txs_batch, &mut batch)?;

        let number = block.number();
        let block_hash = block.hash();

        let block_by_number = (number.into(), block.into());
        self.blocks_by_number.prepare_batch_insertion([block_by_number], &mut batch)?;

        let block_by_hash = (block_hash.into(), number.into());
        self.blocks_by_hash.prepare_batch_insertion([block_by_hash], &mut batch)?;

        self.prepare_batch_with_execution_changes(account_changes, number, &mut batch)?;

        self.accounts.prepare_batch_insertion(
            accounts.iter().cloned().map(|acc| {
                let tup = <(AddressRocksdb, AccountRocksdb)>::from(acc);
                (tup.0, tup.1.into())
            }),
            &mut batch,
        )?;

        self.accounts_history.prepare_batch_insertion(
            accounts.iter().cloned().map(|acc| {
                let tup = <(AddressRocksdb, AccountRocksdb)>::from(acc);
                ((tup.0, 0u32.into()), tup.1.into())
            }),
            &mut batch,
        )?;

        self.write_in_batch_for_multiple_cfs(batch)
    }

    pub fn save_block(&self, block: Block, account_changes: ExecutionChanges) -> Result<()> {
        let mut batch = WriteBatch::default();
        self.prepare_block_insertion(block, account_changes, &mut batch)?;
        self.write_in_batch_for_multiple_cfs(batch)
    }

    pub fn prepare_block_insertion(&self, block: Block, account_changes: ExecutionChanges, batch: &mut WriteBatch) -> Result<()> {
        let mut txs_batch = vec![];
        for transaction in block.transactions.iter().cloned() {
            txs_batch.push((transaction.input.hash.into(), transaction.block_number.into()));
        }

        self.transactions.prepare_batch_insertion(txs_batch, batch)?;

        let number = block.number();
        let block_hash = block.hash();

        let block_by_number = (number.into(), block.into());
        self.blocks_by_number.prepare_batch_insertion([block_by_number], batch)?;

        let block_by_hash = (block_hash.into(), number.into());
        self.blocks_by_hash.prepare_batch_insertion([block_by_hash], batch)?;

        self.prepare_batch_with_execution_changes(account_changes, number, batch)?;

        Ok(())
    }

    /// Write to DB in a batch
    pub fn write_in_batch_for_multiple_cfs(&self, batch: WriteBatch) -> Result<()> {
        tracing::debug!("writing batch");
        let batch_len = batch.len();

        let mut options = WriteOptions::default();
        // By default, each write to rocksdb is asynchronous: it returns after pushing
        // the write from the process into the operating system (buffer cache).
        //
        // This option offers the trade-off of waiting after each write to
        // ensure data is persisted to disk before returning, preventing
        // potential data loss in case of system failure.
        options.set_sync(self.enable_sync_write);

        self.db
            .write_opt(batch, &options)
            .context("failed to write in batch to (possibly) multiple column families")
            .inspect_err(|e| {
                tracing::error!(reason = ?e, batch_len, "failed to write batch to DB");
            })
    }

    #[cfg(feature = "dev")]
    pub fn save_slot(&self, address: Address, slot: Slot) -> Result<()> {
        let mut batch = WriteBatch::default();
        self.account_slots
            .prepare_batch_insertion([((address.into(), slot.index.into()), slot.value.into())], &mut batch)?;
        self.account_slots.apply_batch_with_context(batch)
    }

    #[cfg(feature = "dev")]
    pub fn save_account_nonce(&self, address: Address, nonce: Nonce) -> Result<()> {
        let mut batch = WriteBatch::default();

        // Get the current account or create a new one
        let mut account_info_entry = self.accounts.get(&address.into())?.unwrap_or(AccountRocksdb::default().into());

        // Update the nonce
        account_info_entry.nonce = nonce.into();

        // Save the account
        self.accounts.prepare_batch_insertion([(address.into(), account_info_entry)], &mut batch)?;
        self.accounts.apply_batch_with_context(batch)
    }

    #[cfg(feature = "dev")]
    pub fn save_account_balance(&self, address: Address, balance: Wei) -> Result<()> {
        let mut batch = WriteBatch::default();

        // Get the current account or create a new one
        let mut account_info_entry = self.accounts.get(&address.into())?.unwrap_or(AccountRocksdb::default().into());

        // Update the balance
        account_info_entry.balance = balance.into();

        // Save the account
        self.accounts.prepare_batch_insertion([(address.into(), account_info_entry)], &mut batch)?;
        self.accounts.apply_batch_with_context(batch)
    }

    #[cfg(feature = "dev")]
    pub fn save_account_code(&self, address: Address, code: Bytes) -> Result<()> {
        use crate::alias::RevmBytecode;

        let mut batch = WriteBatch::default();

        // Get the current account or create a new one
        let mut account_info_entry = self.accounts.get(&address.into())?.unwrap_or(AccountRocksdb::default().into());

        // Update the bytecode
        account_info_entry.bytecode = if code.0.is_empty() {
            None
        } else {
            Some(RevmBytecode::new_raw(code.0.into()))
        }
        .map_into();

        // Save the account
        self.accounts.prepare_batch_insertion([(address.into(), account_info_entry)], &mut batch)?;
        self.accounts.apply_batch_with_context(batch)
    }

    #[cfg(test)]
    pub fn read_all_accounts(&self) -> Result<Vec<AccountRocksdb>> {
        self.accounts.iter_start().map(|result| Ok(result?.1.into_inner())).collect()
    }

    #[cfg(test)]
    pub fn read_all_historical_accounts(&self) -> Result<Vec<AccountRocksdb>> {
        self.accounts_history.iter_start().map(|result| Ok(result?.1.into_inner())).collect()
    }

    #[cfg(feature = "dev")]
    pub fn clear(&self) -> Result<()> {
        self.accounts.clear().context("when clearing accounts")?;
        self.accounts_history.clear().context("when clearing accounts_history")?;
        self.account_slots.clear().context("when clearing account_slots")?;
        self.account_slots_history.clear().context("when clearing account_slots_history")?;
        self.transactions.clear().context("when clearing transactions")?;
        self.blocks_by_hash.clear().context("when clearing blocks_by_hash")?;
        self.blocks_by_number.clear().context("when clearing blocks_by_number")?;
        Ok(())
    }

    pub fn read_block_with_changes(&self, selection: BlockFilter) -> Result<Option<(Block, ExecutionChanges)>> {
        let Some(block_wo_changes) = self.read_block(selection)? else {
            return Ok(None);
        };
        let changes = self.block_changes.get(&block_wo_changes.number().into())?;
        Ok(Some((block_wo_changes, changes.map(|changes| changes.into_inner().into()).unwrap_or_default())))
    }
}

#[cfg(feature = "rocks_metrics")]
impl RocksStorageState {
    pub fn export_metrics(&self) -> Result<()> {
        let db_get = self.db_options.get_histogram_data(Histogram::DbGet);
        let db_write = self.db_options.get_histogram_data(Histogram::DbWrite);

        let wal_file_synced = self.db_options.get_ticker_count(Ticker::WalFileSynced);
        let block_cache_miss = self.db_options.get_ticker_count(Ticker::BlockCacheMiss);
        let block_cache_hit = self.db_options.get_ticker_count(Ticker::BlockCacheHit);
        let bytes_written = self.db_options.get_ticker_count(Ticker::BytesWritten);
        let bytes_read = self.db_options.get_ticker_count(Ticker::BytesRead);

        let cur_size_active_mem_table = self.db.property_int_value(rocksdb::properties::CUR_SIZE_ACTIVE_MEM_TABLE).unwrap_or_default();
        let cur_size_all_mem_tables = self.db.property_int_value(rocksdb::properties::CUR_SIZE_ALL_MEM_TABLES).unwrap_or_default();
        let size_all_mem_tables = self.db.property_int_value(rocksdb::properties::SIZE_ALL_MEM_TABLES).unwrap_or_default();
        let block_cache_usage = self.db.property_int_value(rocksdb::properties::BLOCK_CACHE_USAGE).unwrap_or_default();
        let block_cache_capacity = self.db.property_int_value(rocksdb::properties::BLOCK_CACHE_CAPACITY).unwrap_or_default();
        let background_errors = self.db.property_int_value(rocksdb::properties::BACKGROUND_ERRORS).unwrap_or_default();

        let db_name = self.db_path_filename();

        metrics::set_rocks_db_get(db_get.count(), db_name);
        metrics::set_rocks_db_write(db_write.count(), db_name);
        metrics::set_rocks_block_cache_miss(block_cache_miss, db_name);
        metrics::set_rocks_block_cache_hit(block_cache_hit, db_name);
        metrics::set_rocks_bytes_written(bytes_written, db_name);
        metrics::set_rocks_bytes_read(bytes_read, db_name);
        metrics::set_rocks_wal_file_synced(wal_file_synced, db_name);

        metrics::set_rocks_compaction_time(self.get_histogram_average_in_interval(Histogram::CompactionTime)?, db_name);
        metrics::set_rocks_compaction_cpu_time(self.get_histogram_average_in_interval(Histogram::CompactionCpuTime)?, db_name);
        metrics::set_rocks_flush_time(self.get_histogram_average_in_interval(Histogram::FlushTime)?, db_name);

        if let Some(cur_size_active_mem_table) = cur_size_active_mem_table {
            metrics::set_rocks_cur_size_active_mem_table(cur_size_active_mem_table, db_name);
        }

        if let Some(cur_size_all_mem_tables) = cur_size_all_mem_tables {
            metrics::set_rocks_cur_size_all_mem_tables(cur_size_all_mem_tables, db_name);
        }

        if let Some(size_all_mem_tables) = size_all_mem_tables {
            metrics::set_rocks_size_all_mem_tables(size_all_mem_tables, db_name);
        }

        if let Some(block_cache_usage) = block_cache_usage {
            metrics::set_rocks_block_cache_usage(block_cache_usage, db_name);
        }
        if let Some(block_cache_capacity) = block_cache_capacity {
            metrics::set_rocks_block_cache_capacity(block_cache_capacity, db_name);
        }
        if let Some(background_errors) = background_errors {
            metrics::set_rocks_background_errors(background_errors, db_name);
        }

        self.account_slots.export_metrics();
        self.account_slots_history.export_metrics();
        self.accounts.export_metrics();
        self.accounts_history.export_metrics();
        self.blocks_by_hash.export_metrics();
        self.blocks_by_number.export_metrics();
        self.transactions.export_metrics();
        Ok(())
    }

    fn get_histogram_average_in_interval(&self, hist: Histogram) -> Result<u64> {
        // The stats are cumulative since opening the db
        // we can get the average in the time interval with: avg = (new_sum - sum)/(new_count - count)

        let mut prev_values = self.prev_stats.lock();

        let (prev_sum, prev_count): (Sum, Count) = *prev_values.get(&(hist as u32)).unwrap_or(&(0, 0));
        let data = self.db_options.get_histogram_data(hist);
        let data_count = data.count();
        let data_sum = data.sum();

        let Some(avg) = (data_sum - prev_sum).checked_div(data_count - prev_count) else {
            return Ok(0);
        };

        prev_values.insert(hist as u32, (data_sum, data_count));
        Ok(avg)
    }
}

#[cfg(feature = "metrics")]
impl RocksStorageState {
    pub fn export_column_family_size_metrics(&self) -> Result<()> {
        let db_name = self.db_path_filename();

        let cf_names = DB::list_cf(&Options::default(), &self.db_path)?;

        for cf_name in cf_names {
            if let Some(cf_handle) = self.db.cf_handle(&cf_name)
                && let Ok(Some(size)) = self.db.property_int_value_cf(&cf_handle, "rocksdb.total-sst-files-size")
            {
                metrics::set_rocks_cf_size(size, db_name, &cf_name);
            }
        }

        Ok(())
    }
}

impl Drop for RocksStorageState {
    fn drop(&mut self) {
        let mut options = WaitForCompactOptions::default();
        // if background jobs are paused, it makes no sense to keep waiting indefinitely
        options.set_abort_on_pause(true);
        // flush all write buffers before waiting
        options.set_flush(true);
        // wait for the duration passed to `--rocks-shutdown-timeout`, or the default value
        options.set_timeout(self.shutdown_timeout.as_micros().try_into().unwrap_or_default());

        tracing::info!("starting rocksdb shutdown");
        let instant = Instant::now();

        // here, waiting for is done to force WAL logs to be processed, to skip replaying most of them when booting
        // up, which was slowing things down, we are sacrificing a long shutdown for a faster boot
        let result = self.db.wait_for_compact(&options);
        let waited_for = instant.elapsed();

        #[cfg(feature = "metrics")]
        {
            let db_name = self.db_path_filename();
            metrics::set_rocks_last_shutdown_delay_millis(waited_for.as_millis() as u64, db_name);
        }

        if let Err(e) = result {
            tracing::error!(reason = ?e, ?waited_for, "rocksdb shutdown compaction didn't finish in time, shutting it down anyways");
        } else {
            tracing::info!(?waited_for, "finished rocksdb shutdown");
        }
    }
}

impl fmt::Debug for RocksStorageState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RocksStorageState").field("db_path", &self.db_path).finish()
    }
}

#[cfg(test)]
mod tests {

    use fake::Fake;
    use fake::Faker;

    use super::*;
    use crate::alias::RevmBytecode;
    use crate::eth::primitives::BlockHeader;
    use crate::eth::primitives::ExecutionAccountChanges;
    use crate::eth::primitives::ExecutionValueChange;

    #[test]
    #[cfg(feature = "dev")]
    fn test_rocks_multi_get() {
        use std::collections::HashMap;
        use std::collections::HashSet;
        use std::iter;

        type Key = (AddressRocksdb, SlotIndexRocksdb);
        type Value = CfAccountSlotsValue;

        let (mut state, _test_dir) = RocksStorageState::new_in_testdir().unwrap();
        let account_slots = &mut state.account_slots;

        // slots and extra_slots to be inserted
        let slots: HashMap<Key, Value> = (0..1000).map(|_| Faker.fake()).collect();
        let extra_slots: HashMap<Key, Value> = iter::repeat_with(|| Faker.fake())
            .filter(|(key, _)| !slots.contains_key(key))
            .take(1000)
            .collect();

        let mut batch = WriteBatch::default();
        account_slots.prepare_batch_insertion(slots.clone(), &mut batch).unwrap();
        account_slots.prepare_batch_insertion(extra_slots.clone(), &mut batch).unwrap();
        account_slots.apply_batch_with_context(batch).unwrap();

        // keys that don't match any entry
        let extra_keys: HashSet<Key> = (0..1000)
            .map(|_| Faker.fake())
            .filter(|key| !extra_slots.contains_key(key) && !slots.contains_key(key))
            .collect();

        // concatenation of keys for inserted elements, and keys that aren't in the DB
        let keys: Vec<Key> = slots.keys().copied().chain(extra_keys).collect();
        let result = account_slots.multi_get(keys).expect("this should not fail");

        // check that, besides having extra slots inserted, and extra keys when querying,
        // only the expected slots are returned
        assert_eq!(result.len(), slots.keys().len());
        for (idx, value) in result {
            assert_eq!(value, *slots.get(&idx).expect("slot should be found"));
        }
    }

    #[test]
    fn regression_test_read_logs_without_providing_filter_address() {
        let (state, _test_dir) = RocksStorageState::new_in_testdir().unwrap();

        assert_eq!(state.read_logs(&LogFilter::default()).unwrap(), vec![]);

        // 100 blocks with 1 transaction, with 2 logs, total: 200 logs
        for number in 0..100 {
            let block = Block {
                header: BlockHeader {
                    number: number.into(),
                    ..Faker.fake()
                },
                transactions: vec![TransactionMined {
                    block_number: number.into(),
                    logs: vec![Faker.fake(), Faker.fake()],
                    ..Faker.fake()
                }],
            };

            state.save_block(block, ExecutionChanges::default()).unwrap();
        }

        let filter = LogFilter {
            // address is empty
            addresses: vec![],
            ..Default::default()
        };

        assert_eq!(state.read_logs(&filter).unwrap().len(), 200);
    }

    #[test]
    fn test_column_families_creation_order_is_deterministic() {
        let mut previous_order: Option<Vec<String>> = None;

        // Run the test multiple times to ensure RocksDB CF creation order is deterministic
        for i in 0..10 {
            let test_dir = tempfile::tempdir().unwrap();
            let path = test_dir.as_ref().display().to_string();

            // Create the database with the column families
            let cf_options_map = generate_cf_options_map(None);
            let (_db, _) = create_or_open_db(&path, &cf_options_map).unwrap();

            // Get the actual CF order from RocksDB
            let actual_cf_order = DB::list_cf(&Options::default(), &path).unwrap();

            // Compare with previous iteration
            if let Some(prev_order) = &previous_order {
                assert_eq!(
                    &actual_cf_order,
                    prev_order,
                    "RocksDB column family order changed between iterations {} and {}",
                    i - 1,
                    i
                );
            } else {
                previous_order = Some(actual_cf_order);
            }
        }
    }

    #[test]
    fn test_bincode_lexicographical_ordering() {
        use super::super::types::BlockNumberRocksdb;
        use crate::rocks_bincode_config;

        let boundary_tests = vec![(250, 251), (255, 256), (511, 512), (1023, 1024), (2047, 2048)];

        for (before, after) in boundary_tests {
            let before_rocks = BlockNumberRocksdb::from(before as u32);
            let after_rocks = BlockNumberRocksdb::from(after as u32);

            let before_bytes = bincode::encode_to_vec(before_rocks, rocks_bincode_config()).unwrap();
            let after_bytes = bincode::encode_to_vec(after_rocks, rocks_bincode_config()).unwrap();

            let lexicographic_order_correct = before_bytes < after_bytes;

            assert!(
                lexicographic_order_correct,
                "Block {before} should serialize to bytes < Block {after} for RocksDB ordering",
            );
        }
    }
}
