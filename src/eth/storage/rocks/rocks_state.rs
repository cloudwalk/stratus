use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use lazy_static::lazy_static;
use rocksdb::Direction;
use rocksdb::Options;
use rocksdb::WriteBatch;
use rocksdb::DB;
use serde::Deserialize;
use serde::Serialize;
use sugars::hmap;

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
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::rocks::types::AccountRocksdb;
use crate::eth::storage::rocks::types::AddressRocksdb;
use crate::eth::storage::rocks::types::BlockNumberRocksdb;
use crate::eth::storage::rocks::types::BlockRocksdb;
use crate::eth::storage::rocks::types::HashRocksdb;
use crate::eth::storage::rocks::types::IndexRocksdb;
use crate::eth::storage::rocks::types::SlotIndexRocksdb;
use crate::eth::storage::rocks::types::SlotValueRocksdb;
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

/// Helper for creating a `RocksCfRef` with our option presets.
fn new_cf_ref<K, V>(db: &Arc<DB>, column_family: &str) -> RocksCfRef<K, V>
where
    K: Serialize + for<'de> Deserialize<'de> + std::hash::Hash + Eq,
    V: Serialize + for<'de> Deserialize<'de> + Clone,
{
    tracing::debug!(column_family = column_family, "creating new column family");
    let Some(options) = CF_OPTIONS_MAP.get(column_family) else {
        panic!("column_family `{column_family}` given to `new_cf_ref` not found in config options map");
    };
    // NOTE: this doesn't create the CF in the database, read `RocksCfRef` docs for details
    RocksCfRef::new(Arc::clone(db), column_family, options.clone())
}

/// State handler for our RocksDB storage, separating "tables" by column families.
///
/// With data separated by column families, writing and reading should be done via the `RocksCfRef` fields.
pub struct RocksStorageState {
    db: Arc<DB>,
    db_path: PathBuf,
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
    pub fn new(path: impl AsRef<Path>) -> Self {
        let db_path = path.as_ref().to_path_buf();

        tracing::debug!("creating (or opening an existing) database with the specified column families");
        #[cfg_attr(not(feature = "metrics"), allow(unused_variables))]
        let (db, db_options) = create_or_open_db(&db_path, &CF_OPTIONS_MAP).unwrap();

        tracing::debug!("opened database successfully");
        let state = Self {
            db_path,
            accounts: new_cf_ref(&db, "accounts"),
            accounts_history: new_cf_ref(&db, "accounts_history"),
            account_slots: new_cf_ref(&db, "account_slots"),
            account_slots_history: new_cf_ref(&db, "account_slots_history"),
            transactions: new_cf_ref(&db, "transactions"),
            blocks_by_number: new_cf_ref(&db, "blocks_by_number"),
            blocks_by_hash: new_cf_ref(&db, "blocks_by_hash"),
            logs: new_cf_ref(&db, "logs"),
            #[cfg(feature = "metrics")]
            prev_stats: Default::default(),
            #[cfg(feature = "metrics")]
            db_options,
            db,
        };

        tracing::debug!("returning RocksStorageState");
        state
    }

    pub fn preload_block_number(&self) -> anyhow::Result<AtomicU64> {
        let block_number = self.blocks_by_number.last_key().unwrap_or_default();
        tracing::info!(%block_number, "preloaded block_number");
        Ok((u64::from(block_number)).into())
    }

    pub fn reset_at(&self, block_number: BlockNumberRocksdb) -> anyhow::Result<()> {
        // Clear current state
        self.account_slots.clear().unwrap();
        self.accounts.clear().unwrap();

        // Get current state back from historical
        let mut latest_slots: HashMap<(AddressRocksdb, SlotIndexRocksdb), (BlockNumberRocksdb, SlotValueRocksdb)> = HashMap::new();
        let mut latest_accounts: HashMap<AddressRocksdb, (BlockNumberRocksdb, AccountRocksdb)> = HashMap::new();
        for ((address, idx, block), value) in self.account_slots_history.iter_start() {
            if block > block_number {
                self.account_slots_history.delete(&(address, idx, block)).unwrap();
            } else if let Some((bnum, _)) = latest_slots.get(&(address, idx)) {
                if bnum < &block {
                    latest_slots.insert((address, idx), (block, value.clone()));
                }
            } else {
                latest_slots.insert((address, idx), (block, value.clone()));
            }
        }
        for ((address, block), account) in self.accounts_history.iter_start() {
            if block > block_number {
                self.accounts_history.delete(&(address, block)).unwrap();
            } else if let Some((bnum, _)) = latest_accounts.get(&address) {
                if bnum < &block {
                    latest_accounts.insert(address, (block, account)).unwrap();
                }
            } else {
                latest_accounts.insert(address, (block, account));
            }
        }

        // write new current state
        let mut batch = WriteBatch::default();
        let accounts_iter = latest_accounts.into_iter().map(|(address, (_, account))| (address, account));
        self.accounts.prepare_batch_insertion(accounts_iter, &mut batch);
        let slots_iter = latest_slots.into_iter().map(|((address, idx), (_, value))| ((address, idx), value));
        self.account_slots.prepare_batch_insertion(slots_iter, &mut batch);
        self.write_batch(batch).unwrap();

        // Truncate rest of
        for (hash, block) in self.transactions.iter_start() {
            if block > block_number {
                self.transactions.delete(&hash).unwrap();
            }
        }

        for (key, block) in self.logs.iter_start() {
            if block > block_number {
                self.logs.delete(&key).unwrap();
            }
        }

        for (hash, block) in self.blocks_by_hash.iter_start() {
            if block > block_number {
                self.blocks_by_hash.delete(&hash).unwrap();
            }
        }

        for (block, _) in self.blocks_by_number.iter_end() {
            if block > block_number {
                self.blocks_by_number.delete(&block).unwrap();
            } else {
                break;
            }
        }

        Ok(())
    }

    /// Updates the in-memory state with changes from transaction execution
    fn prepare_batch_state_update_with_execution_changes(&self, changes: &[ExecutionAccountChanges], block_number: BlockNumber, batch: &mut WriteBatch) {
        let accounts = self.accounts.clone();
        let accounts_history = self.accounts_history.clone();
        let account_slots = self.account_slots.clone();
        let account_slots_history = self.account_slots_history.clone();

        let mut account_changes = Vec::new();
        let mut account_history_changes = Vec::new();

        for change in changes {
            let address: AddressRocksdb = change.address.into();
            let mut account_info_entry = accounts.get_or_insert_with(address, AccountRocksdb::default);

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
            account_history_changes.push(((address, block_number.into()), account_info_entry));
        }

        accounts.prepare_batch_insertion(account_changes, batch);
        accounts_history.prepare_batch_insertion(account_history_changes, batch);

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
        account_slots.prepare_batch_insertion(slot_changes, batch);
        account_slots_history.prepare_batch_insertion(slot_history_changes, batch);
    }

    pub fn read_transaction(&self, tx_hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        match self.transactions.get(&(*tx_hash).into()) {
            Some(block_number) => match self.blocks_by_number.get(&block_number) {
                Some(block) => {
                    tracing::trace!(%tx_hash, "transaction found");
                    match block.transactions.into_iter().find(|tx| &Hash::from(tx.input.hash) == tx_hash) {
                        Some(tx) => Ok(Some(tx.into())),
                        None => log_and_err!("transaction was not found in block")
                            .with_context(|| format!("block_number = {:?} tx_hash = {}", block_number, tx_hash)),
                    }
                }
                None => log_and_err!("the block that the transaction was supposed to be in was not found")
                    .with_context(|| format!("block_number = {:?} tx_hash = {}", block_number, tx_hash)),
            },
            None => Ok(None),
        }
    }

    pub fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        let addresses: HashSet<AddressRocksdb> = filter.addresses.iter().map(|&address| AddressRocksdb::from(address)).collect();

        let end_block_range_filter = |number: BlockNumber| match filter.to_block.as_ref() {
            Some(&last_block) => number <= last_block,
            None => true,
        };

        Ok(self
            .blocks_by_number
            .iter_from(BlockNumberRocksdb::from(filter.from_block), Direction::Forward)
            .take_while(|(number, _)| end_block_range_filter((*number).into()))
            .flat_map(|(_, block)| block.transactions)
            .filter(|transaction| transaction.input.to.is_some_and(|to| addresses.contains(&to)))
            .flat_map(|transaction| transaction.logs)
            .map(LogMined::from)
            .filter(|log_mined| filter.matches(log_mined))
            .collect())
    }

    pub fn read_slot(&self, address: &Address, index: &SlotIndex, point_in_time: &StoragePointInTime) -> Option<Slot> {
        if address.is_coinbase() {
            //XXX temporary, we will reload the database later without it
            return None;
        }

        match point_in_time {
            StoragePointInTime::Mined | StoragePointInTime::Pending =>
                self.account_slots.get(&((*address).into(), (*index).into())).map(|account_slot_value| Slot {
                    index: *index,
                    value: account_slot_value.clone().into(),
                }),
            StoragePointInTime::MinedPast(number) => {
                if let Some(((rocks_address, rocks_index, _), value)) = self
                    .account_slots_history
                    .iter_from((*address, *index, *number), rocksdb::Direction::Reverse)
                    .next()
                {
                    if rocks_index == (*index).into() && rocks_address == (*address).into() {
                        return Some(Slot {
                            index: rocks_index.into(),
                            value: value.into(),
                        });
                    }
                }
                None
            }
        }
    }

    pub fn read_all_slots(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Vec<Slot>> {
        let rocks_address: AddressRocksdb = (*address).into();

        let present_slots = self
            .account_slots
            .iter_from((rocks_address, SlotIndexRocksdb::from(0)), rocksdb::Direction::Forward)
            .take_while(|((addr, _), _)| &rocks_address == addr)
            .map(|((_, idx), value)| Slot {
                index: idx.into(),
                value: value.into(),
            })
            .collect();

        match point_in_time {
            StoragePointInTime::Mined | StoragePointInTime::Pending => Ok(present_slots),
            StoragePointInTime::MinedPast(_) => {
                let mut past_slots = Vec::with_capacity(present_slots.len());
                for index in present_slots.iter().map(|s| s.index) {
                    let past_slot = self.read_slot(address, &index, point_in_time);
                    if let Some(past_slot) = past_slot {
                        past_slots.push(past_slot);
                    }
                }
                Ok(past_slots)
            }
        }
    }

    pub fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> Option<Account> {
        if address.is_coinbase() || address.is_zero() {
            //XXX temporary, we will reload the database later without it
            return None;
        }

        match point_in_time {
            StoragePointInTime::Mined | StoragePointInTime::Pending => match self.accounts.get(&((*address).into())) {
                Some(inner_account) => {
                    let account = inner_account.to_account(address);
                    tracing::trace!(%address, ?account, "account found");
                    Some(account)
                }

                None => {
                    tracing::trace!(%address, "account not found");
                    None
                }
            },
            StoragePointInTime::MinedPast(block_number) => {
                let rocks_address: AddressRocksdb = (*address).into();
                if let Some(((addr, _), account_info)) = self
                    .accounts_history
                    .iter_from((rocks_address, *block_number), rocksdb::Direction::Reverse)
                    .next()
                {
                    if addr == (*address).into() {
                        return Some(account_info.to_account(address));
                    }
                }
                None
            }
        }
    }

    pub fn read_block(&self, selection: &BlockFilter) -> Option<Block> {
        tracing::debug!(?selection, "reading block");

        let block = match selection {
            BlockFilter::Latest | BlockFilter::Pending => self.blocks_by_number.iter_end().next().map(|(_, block)| block),
            BlockFilter::Earliest => self.blocks_by_number.iter_start().next().map(|(_, block)| block),
            BlockFilter::Number(block_number) => self.blocks_by_number.get(&(*block_number).into()),
            BlockFilter::Hash(block_hash) =>
                if let Some(block_number) = self.blocks_by_hash.get(&(*block_hash).into()) {
                    self.blocks_by_number.get(&block_number)
                } else {
                    None
                },
        };
        match block {
            Some(block) => {
                tracing::trace!(?selection, ?block, "block found");
                Some(block.into())
            }
            None => None,
        }
    }

    pub fn save_accounts(&self, accounts: Vec<Account>) {
        for account in accounts {
            let (key, value) = account.into();
            self.accounts.insert(key, value.clone());
            self.accounts_history.insert((key, 0.into()), value);
        }
    }

    pub fn save_block(&self, block: Block) -> anyhow::Result<()> {
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

        self.transactions.prepare_batch_insertion(txs_batch, &mut batch);
        self.logs.prepare_batch_insertion(logs_batch, &mut batch);

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
        self.blocks_by_number.prepare_batch_insertion([block_by_number], &mut batch);

        let block_by_hash = (block_hash.into(), number.into());
        self.blocks_by_hash.prepare_batch_insertion([block_by_hash], &mut batch);

        self.prepare_batch_state_update_with_execution_changes(&account_changes, number, &mut batch);

        self.write_batch(batch).unwrap();
        Ok(())
    }

    /// Writes accounts to state (does not write to account history)
    #[allow(dead_code)]
    fn write_accounts(&self, accounts: Vec<Account>) {
        let accounts = accounts.into_iter().map(Into::into);

        let mut batch = WriteBatch::default();
        self.accounts.prepare_batch_insertion(accounts, &mut batch);
        self.db.write(batch).unwrap();
    }

    /// Writes slots to state (does not write to slot history)
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn write_slots(&self, slots: Vec<(Address, Slot)>) {
        let slots = slots
            .into_iter()
            .map(|(address, slot)| ((address.into(), slot.index.into()), slot.value.into()));

        let mut batch = WriteBatch::default();
        self.account_slots.prepare_batch_insertion(slots, &mut batch);
        self.db.write(batch).unwrap();
    }

    /// Write to all DBs in a batch
    fn write_batch(&self, batch: WriteBatch) -> anyhow::Result<()> {
        let batch_len = batch.len();
        let result = self.db.write(batch);

        if let Err(e) = &result {
            tracing::error!(reason = ?e, batch_len, "failed to write batch to DB");
        }
        result.map_err(Into::into)
    }

    /// Clears in-memory state.
    pub fn clear(&self) -> anyhow::Result<()> {
        self.accounts.clear()?;
        self.accounts_history.clear()?;
        self.account_slots.clear()?;
        self.account_slots_history.clear()?;
        self.transactions.clear()?;
        self.blocks_by_hash.clear()?;
        self.blocks_by_number.clear()?;
        self.logs.clear()?;
        Ok(())
    }
}

#[cfg(feature = "metrics")]
impl RocksStorageState {
    pub fn export_metrics(&self) {
        let db_get = self.db_options.get_histogram_data(Histogram::DbGet);
        let db_write = self.db_options.get_histogram_data(Histogram::DbWrite);

        let block_cache_miss = self.db_options.get_ticker_count(Ticker::BlockCacheMiss);
        let block_cache_hit = self.db_options.get_ticker_count(Ticker::BlockCacheHit);
        let bytes_written = self.db_options.get_ticker_count(Ticker::BytesWritten);
        let bytes_read = self.db_options.get_ticker_count(Ticker::BytesRead);

        let db_name = self.db.path().file_name().unwrap().to_str();

        metrics::set_rocks_db_get(db_get.count(), db_name);
        metrics::set_rocks_db_write(db_write.count(), db_name);
        metrics::set_rocks_block_cache_miss(block_cache_miss, db_name);
        metrics::set_rocks_block_cache_hit(block_cache_hit, db_name);
        metrics::set_rocks_bytes_written(bytes_written, db_name);
        metrics::set_rocks_bytes_read(bytes_read, db_name);

        metrics::set_rocks_compaction_time(self.get_histogram_average_in_interval(Histogram::CompactionTime), db_name);
        metrics::set_rocks_compaction_cpu_time(self.get_histogram_average_in_interval(Histogram::CompactionCpuTime), db_name);
        metrics::set_rocks_flush_time(self.get_histogram_average_in_interval(Histogram::FlushTime), db_name);
    }

    fn get_histogram_average_in_interval(&self, hist: Histogram) -> u64 {
        // The stats are cumulative since opening the db
        // we can get the average in the time interval with: avg = (new_sum - sum)/(new_count - count)

        let mut prev_values = self.prev_stats.lock().unwrap();
        let (prev_sum, prev_count): (Sum, Count) = *prev_values.get(&(hist as u32)).unwrap_or(&(0, 0));
        let data = self.db_options.get_histogram_data(hist);
        let data_count = data.count();
        let data_sum = data.sum();

        let Some(avg) = (data_sum - prev_sum).checked_div(data_count - prev_count) else {
            return 0;
        };

        prev_values.insert(hist as u32, (data_sum, data_count));
        avg
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
        let account_slots: RocksCfRef<SlotIndex, SlotValue> = new_cf_ref(&db, "account_slots");

        let slots: HashMap<SlotIndex, SlotValue> = (0..1000).map(|_| (Faker.fake(), Faker.fake())).collect();
        let extra_slots: HashMap<SlotIndex, SlotValue> = (0..1000)
            .map(|_| (Faker.fake(), Faker.fake()))
            .filter(|(key, _)| !slots.contains_key(key))
            .collect();

        let mut batch = WriteBatch::default();
        account_slots.prepare_batch_insertion(slots.clone(), &mut batch);
        account_slots.prepare_batch_insertion(extra_slots.clone(), &mut batch);
        db.write(batch).unwrap();

        let extra_keys: HashSet<SlotIndex> = (0..1000)
            .map(|_| Faker.fake())
            .filter(|key| !extra_slots.contains_key(key) && !slots.contains_key(key))
            .collect();

        let keys: Vec<SlotIndex> = slots.keys().cloned().chain(extra_keys).collect();
        let result = account_slots.multi_get(keys).expect("this should not fail");

        assert_eq!(result.len(), slots.keys().len());
        for (idx, value) in result {
            assert_eq!(value, *slots.get(&idx).expect("should not be None"));
        }

        fs::remove_dir_all("./data/slots_test.rocksdb").unwrap();
    }
}
