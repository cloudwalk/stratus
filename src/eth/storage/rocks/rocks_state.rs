use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::fmt::Debug;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Instant;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use lazy_static::lazy_static;
use rocksdb::Direction;
use rocksdb::Options;
use rocksdb::WaitForCompactOptions;
use rocksdb::WriteBatch;
use rocksdb::DB;
use serde::Deserialize;
use serde::Serialize;
use sugars::hmap;

use super::cf_versions::CfAccountSlotsHistoryValue;
use super::cf_versions::CfAccountSlotsValue;
use super::cf_versions::CfAccountsHistoryValue;
use super::cf_versions::CfAccountsValue;
use super::cf_versions::CfBlocksByHashValue;
use super::cf_versions::CfBlocksByNumberValue;
use super::cf_versions::CfLogsValue;
use super::cf_versions::CfTransactionsValue;
use super::rocks_cf::RocksCfRef;
use super::rocks_config::CacheSetting;
use super::rocks_config::DbConfig;
use super::rocks_db::create_or_open_db;
use super::types::AccountRocksdb;
use super::types::AddressRocksdb;
use super::types::BlockNumberRocksdb;
use super::types::HashRocksdb;
use super::types::IndexRocksdb;
use super::types::SlotIndexRocksdb;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::StoragePointInTime;
use crate::ext::OptionExt;
use crate::log_and_err;
use crate::utils::GIGABYTE;

cfg_if::cfg_if! {
    if #[cfg(feature = "metrics")] {
        use std::sync::Mutex;

        use rocksdb::statistics::Histogram;
        use rocksdb::statistics::Ticker;

        use crate::infra::metrics::{self, Count, HistogramInt, Sum};
    }
}

lazy_static! {
    /// Map setting presets for each Column Family
    static ref CF_OPTIONS_MAP: HashMap<&'static str, Options> = hmap! {
        "accounts" => DbConfig::Default.to_options(CacheSetting::Enabled(15 * GIGABYTE)),
        "accounts_history" => DbConfig::FastWriteSST.to_options(CacheSetting::Disabled),
        "account_slots" => DbConfig::Default.to_options(CacheSetting::Enabled(45 * GIGABYTE)),
        "account_slots_history" => DbConfig::FastWriteSST.to_options(CacheSetting::Disabled),
        "transactions" => DbConfig::LargeSSTFiles.to_options(CacheSetting::Disabled),
        "blocks_by_number" => DbConfig::LargeSSTFiles.to_options(CacheSetting::Disabled),
        "blocks_by_hash" => DbConfig::LargeSSTFiles.to_options(CacheSetting::Disabled),
        "logs" => DbConfig::LargeSSTFiles.to_options(CacheSetting::Disabled),
    };
}

/// Helper for creating a `RocksCfRef`, aborting if it wasn't declared in our option presets.
fn new_cf_ref<K, V>(db: &Arc<DB>, column_family: &str) -> Result<RocksCfRef<K, V>>
where
    K: Serialize + for<'de> Deserialize<'de> + Debug + std::hash::Hash + Eq,
    V: Serialize + for<'de> Deserialize<'de> + Debug + Clone,
{
    tracing::debug!(column_family = column_family, "creating new column family");

    CF_OPTIONS_MAP
        .get(column_family)
        .with_context(|| format!("matching column_family `{column_family}` given to `new_cf_ref` wasn't found in configuration map"))?;

    // NOTE: this doesn't create the CFs in the database, read `RocksCfRef` docs for details
    RocksCfRef::new(Arc::clone(db), column_family)
}

/// State handler for our RocksDB storage, separating "tables" by column families.
///
/// With data separated by column families, writing and reading should be done via the `RocksCfRef` fields.
pub struct RocksStorageState {
    db: Arc<DB>,
    db_path: String,
    accounts: RocksCfRef<AddressRocksdb, CfAccountsValue>,
    accounts_history: RocksCfRef<(AddressRocksdb, BlockNumberRocksdb), CfAccountsHistoryValue>,
    account_slots: RocksCfRef<(AddressRocksdb, SlotIndexRocksdb), CfAccountSlotsValue>,
    account_slots_history: RocksCfRef<(AddressRocksdb, SlotIndexRocksdb, BlockNumberRocksdb), CfAccountSlotsHistoryValue>,
    transactions: RocksCfRef<HashRocksdb, CfTransactionsValue>,
    blocks_by_number: RocksCfRef<BlockNumberRocksdb, CfBlocksByNumberValue>,
    blocks_by_hash: RocksCfRef<HashRocksdb, CfBlocksByHashValue>,
    logs: RocksCfRef<(HashRocksdb, IndexRocksdb), CfLogsValue>,
    /// Last collected stats for a histogram
    #[cfg(feature = "metrics")]
    prev_stats: Mutex<HashMap<HistogramInt, (Sum, Count)>>,
    /// Options passed at DB creation, stored for metrics
    ///
    /// a newly created `rocksdb::Options` object is unique, with an underlying pointer identifier inside of it, and is used to access
    /// the DB metrics, `Options` can be cloned, but two equal `Options` might not retrieve the same metrics
    #[cfg(feature = "metrics")]
    db_options: Options,
}

impl RocksStorageState {
    pub fn new(path: String) -> Result<Self> {
        tracing::debug!("creating (or opening an existing) database with the specified column families");
        #[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
        let (db, db_options) = create_or_open_db(&path, &CF_OPTIONS_MAP).context("when trying to create (or open) rocksdb")?;

        if db.path().to_str().is_none() {
            bail!("db path doesn't isn't valid UTF-8: {:?}", db.path());
        }
        if db.path().file_name().is_none() {
            bail!("db path doesn't have a file name or isn't valid UTF-8: {:?}", db.path());
        }

        let state = Self {
            db_path: path,
            accounts: new_cf_ref(&db, "accounts")?,
            accounts_history: new_cf_ref(&db, "accounts_history")?,
            account_slots: new_cf_ref(&db, "account_slots")?,
            account_slots_history: new_cf_ref(&db, "account_slots_history")?,
            transactions: new_cf_ref(&db, "transactions")?,
            blocks_by_number: new_cf_ref(&db, "blocks_by_number")?,
            blocks_by_hash: new_cf_ref(&db, "blocks_by_hash")?,
            logs: new_cf_ref(&db, "logs")?,
            #[cfg(feature = "metrics")]
            prev_stats: Mutex::default(),
            #[cfg(feature = "metrics")]
            db_options,
            db,
        };

        tracing::debug!("opened database successfully");
        Ok(state)
    }

    /// Get the filename of the database path.
    ///
    /// Should be checked on creation.
    fn db_path_filename(&self) -> &str {
        self.db.path().file_name().unwrap().to_str().unwrap()
    }

    pub fn preload_block_number(&self) -> Result<AtomicU64> {
        let block_number = self.blocks_by_number.last_key()?.unwrap_or_default();
        tracing::info!(%block_number, "preloaded block_number");
        Ok((u64::from(block_number)).into())
    }

    pub fn reset(&self) -> Result<()> {
        self.accounts.clear()?;
        self.accounts_history.clear()?;
        self.account_slots.clear()?;
        self.account_slots_history.clear()?;
        self.transactions.clear()?;
        self.blocks_by_number.clear()?;
        self.blocks_by_hash.clear()?;
        self.logs.clear()?;
        Ok(())
    }

    /// Updates the in-memory state with changes from transaction execution
    fn prepare_batch_state_update_with_execution_changes(
        &self,
        changes: &[ExecutionAccountChanges],
        block_number: BlockNumber,
        batch: &mut WriteBatch,
    ) -> Result<()> {
        let accounts = self.accounts.clone();
        let accounts_history = self.accounts_history.clone();
        let account_slots = self.account_slots.clone();
        let account_slots_history = self.account_slots_history.clone();

        let mut account_changes: Vec<(_, CfAccountsValue)> = Vec::new();
        let mut account_history_changes: Vec<(_, CfAccountsHistoryValue)> = Vec::new();

        for change in changes {
            let address: AddressRocksdb = change.address.into();
            let mut account_info_entry = accounts.get_or_insert_with(address, || AccountRocksdb::default().into())?;

            if let Some(nonce) = change.nonce.clone().take_modified() {
                account_info_entry.nonce = nonce.into();
            }
            if let Some(balance) = change.balance.clone().take_modified() {
                account_info_entry.balance = balance.into();
            }
            if let Some(bytecode) = change.bytecode.clone().take_modified() {
                account_info_entry.bytecode = bytecode.map_into();
            }

            account_changes.push((address, account_info_entry.clone()));
            account_history_changes.push(((address, block_number.into()), account_info_entry.into_inner().into()));
        }

        accounts.prepare_batch_insertion(account_changes, batch)?;
        accounts_history.prepare_batch_insertion(account_history_changes, batch)?;

        let mut slot_changes = Vec::new();
        let mut slot_history_changes = Vec::new();

        for change in changes {
            for (slot_index, slot_change) in &change.slots {
                if let Some(slot) = slot_change.take_modified_ref() {
                    let address: AddressRocksdb = change.address.into();
                    let slot_index = *slot_index;
                    slot_changes.push(((address, slot_index.into()), slot.value.into()));
                    slot_history_changes.push(((address, slot_index.into(), block_number.into()), slot.value.into()));
                }
            }
        }
        account_slots.prepare_batch_insertion(slot_changes, batch)?;
        account_slots_history.prepare_batch_insertion(slot_history_changes, batch)?;

        Ok(())
    }

    pub fn read_transaction(&self, tx_hash: &Hash) -> Result<Option<TransactionMined>> {
        let Some(block_number) = self.transactions.get(&(*tx_hash).into())? else {
            return Ok(None);
        };

        let Some(block) = self.blocks_by_number.get(&block_number)? else {
            return log_and_err!("the block that the transaction was supposed to be in was not found")
                .with_context(|| format!("block_number = {:?} tx_hash = {}", block_number, tx_hash));
        };

        let transaction = block.into_inner().transactions.into_iter().find(|tx| &Hash::from(tx.input.hash) == tx_hash);

        match transaction {
            Some(tx) => {
                tracing::trace!(%tx_hash, "transaction found");
                Ok(Some(tx.into()))
            }
            None => log_and_err!("rocks error, transaction wasn't found in block where the index pointed at")
                .with_context(|| format!("block_number = {:?} tx_hash = {}", block_number, tx_hash)),
        }
    }

    pub fn read_logs(&self, filter: &LogFilter) -> Result<Vec<LogMined>> {
        let addresses: HashSet<AddressRocksdb> = filter.addresses.iter().map(|&address| AddressRocksdb::from(address)).collect();

        let end_block_range_filter = |number: BlockNumber| match filter.to_block.as_ref() {
            Some(&last_block) => number <= last_block,
            None => true,
        };

        let iter = self
            .blocks_by_number
            .iter_from(BlockNumberRocksdb::from(filter.from_block), Direction::Forward)?;

        let mut logs_result = vec![];

        for next in iter {
            let (number, block) = next?;

            if !end_block_range_filter(number.into()) {
                break;
            }
            let transactions_with_matching_addresses = block
                .into_inner()
                .transactions
                .into_iter()
                .filter(|transaction| transaction.input.to.is_some_and(|to| addresses.contains(&to)));

            let logs = transactions_with_matching_addresses
                .flat_map(|transaction| transaction.logs)
                .map(LogMined::from);

            let filtered_logs = logs.filter(|log| filter.matches(log));
            logs_result.extend(filtered_logs);
        }
        Ok(logs_result)
    }

    pub fn read_slot(&self, address: &Address, index: &SlotIndex, point_in_time: &StoragePointInTime) -> Result<Option<Slot>> {
        if address.is_coinbase() {
            //XXX temporary, we will reload the database later without it
            return Ok(None);
        }

        match point_in_time {
            StoragePointInTime::Mined | StoragePointInTime::Pending => {
                let query_params = ((*address).into(), (*index).into());

                let Some(account_slot_value) = self.account_slots.get(&query_params)? else {
                    return Ok(None);
                };

                Ok(Some(Slot {
                    index: *index,
                    value: account_slot_value.into_inner().into(),
                }))
            }
            StoragePointInTime::MinedPast(number) => {
                let iterator_start = ((*address).into(), (*index).into(), (*number).into());

                if let Some(((rocks_address, rocks_index, _), value)) = self
                    .account_slots_history
                    .iter_from(iterator_start, rocksdb::Direction::Reverse)?
                    .next()
                    .transpose()?
                {
                    if rocks_index == (*index).into() && rocks_address == (*address).into() {
                        return Ok(Some(Slot {
                            index: rocks_index.into(),
                            value: value.into_inner().into(),
                        }));
                    }
                }
                Ok(None)
            }
        }
    }

    pub fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> Result<Option<Account>> {
        if address.is_coinbase() || address.is_zero() {
            //XXX temporary, we will reload the database later without it
            return Ok(None);
        }

        match point_in_time {
            StoragePointInTime::Mined | StoragePointInTime::Pending => {
                let Some(inner_account) = self.accounts.get(&((*address).into()))? else {
                    tracing::trace!(%address, "account not found");
                    return Ok(None);
                };

                let account = inner_account.to_account(address);
                tracing::trace!(%address, ?account, "account found");
                Ok(Some(account))
            }
            StoragePointInTime::MinedPast(block_number) => {
                let iterator_start = ((*address).into(), (*block_number).into());

                if let Some(next) = self.accounts_history.iter_from(iterator_start, rocksdb::Direction::Reverse)?.next() {
                    let ((addr, _), account_info) = next?;
                    if addr == (*address).into() {
                        return Ok(Some(account_info.to_account(address)));
                    }
                }
                Ok(None)
            }
        }
    }

    pub fn read_block(&self, selection: &BlockFilter) -> Result<Option<Block>> {
        tracing::debug!(?selection, "reading block");

        let block = match selection {
            BlockFilter::Latest | BlockFilter::Pending => self.blocks_by_number.last_value(),
            BlockFilter::Earliest => self.blocks_by_number.first_value(),
            BlockFilter::Number(block_number) => self.blocks_by_number.get(&(*block_number).into()),
            BlockFilter::Hash(block_hash) => {
                if let Some(block_number) = self.blocks_by_hash.get(&(*block_hash).into())? {
                    self.blocks_by_number.get(&block_number)
                } else {
                    Ok(None)
                }
            }
        };

        block.map(|block_option| block_option.map(|block| block.into_inner().into()))
    }

    pub fn save_accounts(&self, accounts: Vec<Account>) -> Result<()> {
        for account in accounts {
            let (key, value) = account.into();
            let value: CfAccountsValue = value.into();
            self.accounts.insert(key, value.clone())?;
            self.accounts_history.insert((key, 0u64.into()), value.into_inner().into())?;
        }
        Ok(())
    }

    pub fn save_block(&self, block: Block) -> Result<()> {
        let account_changes = block.compact_account_changes();

        let mut txs_batch = vec![];
        let mut logs_batch = vec![];
        for transaction in block.transactions.clone() {
            txs_batch.push((transaction.input.hash.into(), transaction.block_number.into()));
            for log in transaction.logs {
                logs_batch.push(((transaction.input.hash.into(), log.log_index.into()), transaction.block_number.into()));
            }
        }
        let mut batch = WriteBatch::default();

        self.transactions.prepare_batch_insertion(txs_batch, &mut batch)?;
        self.logs.prepare_batch_insertion(logs_batch, &mut batch)?;

        let number = block.number();
        let block_hash = block.hash();

        // this is an optimization, instead of saving the entire block into the database,
        // remove all discardable account changes
        let block_without_changes = {
            let mut block_mut = block;
            // mutate it
            block_mut.transactions.iter_mut().for_each(|transaction| {
                // checks if it has a contract address to keep, later this will be used to gather deployed_contract_address
                transaction.execution.changes.retain(|_, change| change.bytecode.is_modified());
            });
            block_mut
        };

        let block_by_number = (number.into(), block_without_changes.into());
        self.blocks_by_number.prepare_batch_insertion([block_by_number], &mut batch)?;

        let block_by_hash = (block_hash.into(), number.into());
        self.blocks_by_hash.prepare_batch_insertion([block_by_hash], &mut batch)?;

        self.prepare_batch_state_update_with_execution_changes(&account_changes, number, &mut batch)?;

        self.write_in_batch_for_multiple_cfs(batch)?;
        Ok(())
    }

    /// Write to DB in a batch
    pub fn write_in_batch_for_multiple_cfs(&self, batch: WriteBatch) -> Result<()> {
        let batch_len = batch.len();
        let result = self.db.write(batch).context("failed to write in batch to multiple column families");

        result.inspect_err(|e| {
            tracing::error!(reason = ?e, batch_len, "failed to write batch to DB");
        })
    }

    /// Writes accounts to state (does not write to account history)
    #[allow(dead_code)]
    fn write_accounts(&self, accounts: Vec<Account>) -> Result<()> {
        let accounts = accounts.into_iter().map(|account| {
            let (address, account) = account.into();
            (address, account.into())
        });

        let mut batch = WriteBatch::default();
        self.accounts.prepare_batch_insertion(accounts, &mut batch)?;
        self.accounts.write_batch_with_context(batch)
    }

    /// Writes slots to state (does not write to slot history)
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn write_slots(&self, slots: Vec<(Address, Slot)>) -> Result<()> {
        let slots = slots
            .into_iter()
            .map(|(address, slot)| ((address.into(), slot.index.into()), slot.value.into()));

        let mut batch = WriteBatch::default();
        self.account_slots.prepare_batch_insertion(slots, &mut batch)?;
        self.account_slots.write_batch_with_context(batch)
    }

    /// Clears in-memory state.
    pub fn clear(&self) -> Result<()> {
        self.accounts.clear().context("when clearing accounts")?;
        self.accounts_history.clear().context("when clearing accounts_history")?;
        self.account_slots.clear().context("when clearing account_slots")?;
        self.account_slots_history.clear().context("when clearing account_slots_history")?;
        self.transactions.clear().context("when clearing transactions")?;
        self.blocks_by_hash.clear().context("when clearing blocks_by_hash")?;
        self.blocks_by_number.clear().context("when clearing blocks_by_number")?;
        self.logs.clear().context("when clearing logs")?;
        Ok(())
    }
}

#[cfg(feature = "metrics")]
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
        self.logs.export_metrics();
        self.transactions.export_metrics();
        Ok(())
    }

    fn get_histogram_average_in_interval(&self, hist: Histogram) -> Result<u64> {
        // The stats are cumulative since opening the db
        // we can get the average in the time interval with: avg = (new_sum - sum)/(new_count - count)

        let Ok(mut prev_values) = self.prev_stats.lock() else {
            bail!("mutex in get_histogram_average_in_interval is poisoned")
        };
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

impl Drop for RocksStorageState {
    fn drop(&mut self) {
        let minute_in_micros = 60 * 1_000_000;

        let mut options = WaitForCompactOptions::default();
        // if background jobs are paused, it makes no sense to keep waiting indefinitely
        options.set_abort_on_pause(true);
        // flush all write buffers before waiting
        options.set_flush(true);
        // wait for 4 minutes at max, sacrificing a long shutdown for a faster boot
        options.set_timeout(4 * minute_in_micros);

        tracing::info!("starting rocksdb shutdown");
        let instant = Instant::now();

        // here, waiting for is done to force WAL logs to be processed, to skip replaying most of them when booting
        // up, which was slowing things down
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
    use std::fs;

    use fake::Fake;
    use fake::Faker;

    use super::*;
    use crate::eth::primitives::SlotValue;

    #[test]
    fn test_rocks_multi_get() {
        let (db, _db_options) = create_or_open_db("./data/slots_test.rocksdb", &CF_OPTIONS_MAP).unwrap();
        let account_slots: RocksCfRef<SlotIndex, SlotValue> = new_cf_ref(&db, "account_slots").unwrap();

        let slots: HashMap<SlotIndex, SlotValue> = (0..1000).map(|_| (Faker.fake(), Faker.fake())).collect();
        let extra_slots: HashMap<SlotIndex, SlotValue> = (0..1000)
            .map(|_| (Faker.fake(), Faker.fake()))
            .filter(|(key, _)| !slots.contains_key(key))
            .collect();

        let mut batch = WriteBatch::default();
        account_slots.prepare_batch_insertion(slots.clone(), &mut batch).unwrap();
        account_slots.prepare_batch_insertion(extra_slots.clone(), &mut batch).unwrap();
        db.write(batch).unwrap();

        let extra_keys: HashSet<SlotIndex> = (0..1000)
            .map(|_| Faker.fake())
            .filter(|key| !extra_slots.contains_key(key) && !slots.contains_key(key))
            .collect();

        let keys: Vec<SlotIndex> = slots.keys().copied().chain(extra_keys).collect();
        let result = account_slots.multi_get(keys).expect("this should not fail");

        assert_eq!(result.len(), slots.keys().len());
        for (idx, value) in result {
            assert_eq!(value, *slots.get(&idx).expect("should not be None"));
        }

        fs::remove_dir_all("./data/slots_test.rocksdb").unwrap();
    }
}
