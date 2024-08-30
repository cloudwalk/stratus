use std::collections::HashMap;
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

use super::rocks_batch_writer::write_in_batch_for_multiple_cfs_impl;
use super::rocks_batch_writer::BufferedBatchWriter;
use super::rocks_cf::RocksCfRef;
use super::rocks_config::CacheSetting;
use super::rocks_config::DbConfig;
use super::rocks_db::create_or_open_db;
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
use crate::eth::storage::rocks::types::AccountRocksdb;
use crate::eth::storage::rocks::types::AddressRocksdb;
use crate::eth::storage::rocks::types::BlockNumberRocksdb;
use crate::eth::storage::rocks::types::BlockRocksdb;
use crate::eth::storage::rocks::types::HashRocksdb;
use crate::eth::storage::rocks::types::IndexRocksdb;
use crate::eth::storage::rocks::types::SlotIndexRocksdb;
use crate::eth::storage::rocks::types::SlotValueRocksdb;
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
    accounts: RocksCfRef<AddressRocksdb, AccountRocksdb>,
    accounts_history: RocksCfRef<(AddressRocksdb, BlockNumberRocksdb), AccountRocksdb>,
    account_slots: RocksCfRef<(AddressRocksdb, SlotIndexRocksdb), SlotValueRocksdb>,
    account_slots_history: RocksCfRef<(AddressRocksdb, SlotIndexRocksdb, BlockNumberRocksdb), SlotValueRocksdb>,
    transactions: RocksCfRef<HashRocksdb, BlockNumberRocksdb>,
    blocks_by_number: RocksCfRef<BlockNumberRocksdb, BlockRocksdb>,
    blocks_by_hash: RocksCfRef<HashRocksdb, BlockNumberRocksdb>,
    logs: RocksCfRef<(HashRocksdb, IndexRocksdb), BlockNumberRocksdb>,
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
    #[cfg(feature = "metrics")]
    fn db_path_filename(&self) -> &str {
        self.db_path.rsplit('/').next().unwrap_or(&self.db_path)
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
    fn prepare_batch_with_execution_changes<C>(&self, changes: C, block_number: BlockNumber, batch: &mut WriteBatch) -> Result<()>
    where
        C: IntoIterator<Item = ExecutionAccountChanges>,
    {
        for change in changes {
            let block_number = block_number.into();
            let address: AddressRocksdb = change.address.into();

            if change.is_account_modified() {
                let mut account_info_entry = self.accounts.get_or_insert_with(address, AccountRocksdb::default)?;

                if let Some(nonce) = change.nonce.take_modified() {
                    account_info_entry.nonce = nonce.into();
                }
                if let Some(balance) = change.balance.take_modified() {
                    account_info_entry.balance = balance.into();
                }
                if let Some(bytecode) = change.bytecode.take_modified() {
                    account_info_entry.bytecode = bytecode.map_into();
                }

                self.accounts.prepare_batch_insertion([(address, account_info_entry.clone())], batch)?;
                self.accounts_history
                    .prepare_batch_insertion([((address, block_number), account_info_entry)], batch)?;
            }

            for (slot_index, slot_change) in &change.slots {
                if let Some(slot) = slot_change.take_modified_ref() {
                    let slot_index = (*slot_index).into();
                    let slot_value = slot.value.into();

                    self.account_slots.prepare_batch_insertion([((address, slot_index), slot_value)], batch)?;
                    self.account_slots_history
                        .prepare_batch_insertion([((address, slot_index, block_number), slot_value)], batch)?;
                }
            }
        }

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

        tracing::trace!(%tx_hash, "transaction found");
        match block.transactions.into_iter().find(|tx| &Hash::from(tx.input.hash) == tx_hash) {
            Some(tx) => Ok(Some(tx.into())),
            None => log_and_err!("rocks error, transaction wasn't found in block where the index pointed at")
                .with_context(|| format!("block_number = {:?} tx_hash = {}", block_number, tx_hash)),
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

            let logs = block.transactions.into_iter().flat_map(|transaction| transaction.logs).map(LogMined::from);

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
                    value: account_slot_value.into(),
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
                            value: value.into(),
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
            BlockFilter::Hash(block_hash) =>
                if let Some(block_number) = self.blocks_by_hash.get(&(*block_hash).into())? {
                    self.blocks_by_number.get(&block_number)
                } else {
                    Ok(None)
                },
        };

        block.map(Option::map_into)
    }

    pub fn save_accounts(&self, accounts: Vec<Account>) -> Result<()> {
        for account in accounts {
            let (key, value) = account.into();
            self.accounts.insert(key, value.clone())?;
            self.accounts_history.insert((key, 0.into()), value)?;
        }
        Ok(())
    }

    pub fn save_block(&self, block: Block) -> Result<()> {
        let account_changes = block.compact_account_changes();

        let mut txs_batch = vec![];
        let mut logs_batch = vec![];
        for transaction in block.transactions.iter().cloned() {
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

        self.prepare_batch_with_execution_changes(account_changes, number, &mut batch)?;

        self.write_in_batch_for_multiple_cfs(batch)?;
        Ok(())
    }

    /// Write to DB in a batch
    pub fn write_in_batch_for_multiple_cfs(&self, batch: WriteBatch) -> Result<()> {
        write_in_batch_for_multiple_cfs_impl(&self.db, batch)
    }

    /// Writes slots to state (does not write to slot history)
    #[cfg(test)]
    pub fn write_slots(&self, slots: Vec<(Address, Slot)>) -> Result<()> {
        let slots = slots
            .into_iter()
            .map(|(address, slot)| ((address.into(), slot.index.into()), slot.value.into()));

        let mut batch = WriteBatch::default();
        self.account_slots.prepare_batch_insertion(slots, &mut batch)?;
        self.account_slots.apply_batch_with_context(batch)
    }

    #[cfg(test)]
    pub fn read_all_accounts(&self) -> Result<Vec<AccountRocksdb>> {
        self.accounts.iter_start().map(|result| Ok(result?.1)).collect()
    }

    #[cfg(test)]
    pub fn read_all_historical_accounts(&self) -> Result<Vec<AccountRocksdb>> {
        self.accounts_history.iter_start().map(|result| Ok(result?.1)).collect()
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

    /// Revert state to target block, removing all effect from blocks after it.
    pub fn revert_state_to_block(&self, target_block: BlockNumberRocksdb) -> Result<()> {
        tracing::info!("clearing current account state (it will be reconstructed)");
        self.accounts.clear()?;
        tracing::info!("clearing current slots state (it will be reconstructed)");
        self.account_slots.clear()?;

        // all data from blocks after `target_block` should be deleted
        let should_delete_block = |block_number| block_number > target_block;

        let mut bufwriter = BufferedBatchWriter::new(1024 * 2);

        struct LastHistoricalAccount {
            address: AddressRocksdb,
            account: AccountRocksdb,
        }

        let mut history_accounts_count = 0_u64;
        let mut accounts_count = 0_u64;
        let mut last_account: Option<LastHistoricalAccount> = None;

        tracing::info!("starting iteration through historical accounts to clean values after target_block and reconstruct current accounts state");
        for next in self.accounts_history.iter_start() {
            let ((address, account_block_number), account) = next?;

            // this can only be `Some` if account in last iteration was in valid range
            if let Some(last) = last_account {
                // if reached a new account, insert the previous as it is the most up-to-date for that address
                let is_different_account = last.address != address;
                // if last was in valid block range, but this one isn't that's the most up-to-date in the valid range
                let is_last_account_before_target_block = should_delete_block(account_block_number);

                if is_last_account_before_target_block || is_different_account {
                    bufwriter.insert(&self.accounts, last.address, last.account)?;
                    accounts_count += 1;
                }
            }

            if should_delete_block(account_block_number) {
                bufwriter.delete(&self.accounts_history, (address, account_block_number))?;
                // skip last_account for next iteration because its block shall be ignored
                last_account = None;
            } else {
                // update last_account for next iteration
                last_account = Some(LastHistoricalAccount { address, account });
            }

            history_accounts_count += 1;
        }

        // always insert last seen account
        if let Some(last) = last_account {
            bufwriter.insert(&self.accounts, last.address, last.account)?;
            accounts_count += 1;
        }

        bufwriter.flush(&self.db)?;
        tracing::info!(%history_accounts_count, %accounts_count, "finished historical accounts iteration");

        struct LastHistoricalSlot {
            address: AddressRocksdb,
            index: SlotIndexRocksdb,
            value: SlotValueRocksdb,
        }

        let mut history_slots_count = 0_u64;
        let mut slots_count = 0_u64;
        let mut last_slot: Option<LastHistoricalSlot> = None;

        tracing::info!("starting iteration through historical slots to clean values after target_block and reconstruct current slots state");
        for next in self.account_slots_history.iter_start() {
            let ((address, index, slot_block_number), value) = next?;

            // this can only be `Some` if slot in last iteration was in valid range
            if let Some(last) = last_slot {
                // if reached a new account, insert the previous as it is the most up-to-date for that address
                let is_different_slot = last.address != address || last.index != index;
                // if last was in valid block range, but this one isn't that's the most up-to-date in the valid range
                let is_last_slot_before_target_block = should_delete_block(slot_block_number);

                if is_last_slot_before_target_block || is_different_slot {
                    bufwriter.insert(&self.account_slots, (last.address, last.index), last.value)?;
                    slots_count += 1;
                }
            }

            if should_delete_block(slot_block_number) {
                bufwriter.delete(&self.account_slots_history, (address, index, slot_block_number))?;
                // skip last_slot for next iteration because its block shall be ignored
                last_slot = None;
            } else {
                // update last_slot for next iteration
                last_slot = Some(LastHistoricalSlot { address, index, value });
            }

            history_slots_count += 1;
        }

        // always insert last seen slot
        if let Some(last) = last_slot {
            bufwriter.insert(&self.account_slots, (last.address, last.index), last.value)?;
            slots_count += 1;
        }

        bufwriter.flush(&self.db)?;
        tracing::info!(%history_slots_count, %slots_count, "finished historical slots iteration");

        tracing::info!("cleaning values in transactions column family");
        for next in self.transactions.iter_start() {
            let (hash, block) = next?;
            if should_delete_block(block) {
                bufwriter.delete(&self.transactions, hash)?;
            }
        }
        bufwriter.flush(&self.db)?;

        tracing::info!("cleaning values in logs column family");
        for next in self.logs.iter_start() {
            let (key, block) = next?;
            if block > target_block {
                bufwriter.delete(&self.logs, key)?;
            }
        }
        bufwriter.flush(&self.db)?;

        tracing::info!("cleaning values in blocks_by_hash column family");
        for next in self.blocks_by_hash.iter_start() {
            let (hash, block) = next?;
            if should_delete_block(block) {
                bufwriter.delete(&self.blocks_by_hash, hash)?;
            }
        }
        bufwriter.flush(&self.db)?;

        tracing::info!("cleaning values in blocks_by_number column family");
        for block in self.blocks_by_number.iter_from(target_block + 1, Direction::Forward)?.keys() {
            bufwriter.delete(&self.blocks_by_number, block?)?;
        }
        bufwriter.flush(&self.db)?;

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
    use std::collections::HashSet;

    use fake::Fake;
    use fake::Faker;
    use tempfile::tempdir;

    use super::*;
    use crate::eth::primitives::BlockHeader;
    use crate::eth::primitives::ExecutionValueChange;
    use crate::eth::primitives::SlotValue;

    #[test]
    fn test_rocks_multi_get() {
        let test_dir = tempdir().unwrap();

        let (db, _db_options) = create_or_open_db(test_dir.path(), &CF_OPTIONS_MAP).unwrap();
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
    }

    #[test]
    fn regression_test_read_logs_without_providing_filter_address() {
        let test_dir = tempdir().unwrap();

        let state = RocksStorageState::new(test_dir.path().display().to_string()).unwrap();

        assert_eq!(state.read_logs(&LogFilter::default()).unwrap(), vec![]);

        // 100 blocks with 1 transaction, with 2 logs, total: 200 logs
        for number in 0..100 {
            let block = Block {
                header: BlockHeader {
                    number: number.into(),
                    ..Faker.fake()
                },
                transactions: vec![TransactionMined {
                    logs: vec![Faker.fake(), Faker.fake()],
                    ..Faker.fake()
                }],
            };

            state.save_block(block).unwrap();
        }

        let filter = LogFilter {
            // address is empty
            addresses: vec![],
            ..Default::default()
        };

        assert_eq!(state.read_logs(&filter).unwrap().len(), 200);
    }

    #[test]
    fn regression_test_saving_account_changes_for_accounts_that_didnt_change() {
        let test_dir = tempdir().unwrap();
        let state = RocksStorageState::new(test_dir.path().display().to_string()).unwrap();

        let change_base = ExecutionAccountChanges {
            new_account: false,
            address: Faker.fake(),
            nonce: ExecutionValueChange::from_original(Faker.fake()),
            balance: ExecutionValueChange::from_original(Faker.fake()),
            bytecode: ExecutionValueChange::from_original(Faker.fake()),
            code_hash: Faker.fake(),
            slots: HashMap::new(),
        };

        // 4 changes for the same address, the first isn't modified, the other three are
        let changes = [
            ExecutionAccountChanges { ..change_base.clone() },
            ExecutionAccountChanges {
                nonce: ExecutionValueChange::from_modified(Faker.fake()),
                ..change_base.clone()
            },
            ExecutionAccountChanges {
                balance: ExecutionValueChange::from_modified(Faker.fake()),
                ..change_base.clone()
            },
            ExecutionAccountChanges {
                bytecode: ExecutionValueChange::from_modified(Faker.fake()),
                ..change_base
            },
        ];

        let mut batch = WriteBatch::default();
        // add accounts in separate blocks so they show up in history
        state.prepare_batch_with_execution_changes([changes[0].clone()], 1.into(), &mut batch).unwrap();
        state.prepare_batch_with_execution_changes([changes[1].clone()], 2.into(), &mut batch).unwrap();
        state.prepare_batch_with_execution_changes([changes[2].clone()], 3.into(), &mut batch).unwrap();
        state.prepare_batch_with_execution_changes([changes[3].clone()], 4.into(), &mut batch).unwrap();
        state.write_in_batch_for_multiple_cfs(batch).unwrap();

        let accounts = state.read_all_accounts().unwrap();
        assert_eq!(accounts.len(), 1);

        let history = state.read_all_historical_accounts().unwrap();
        assert_eq!(history.len(), 3);
    }
}
