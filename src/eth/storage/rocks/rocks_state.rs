use core::fmt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use itertools::Itertools;
use lazy_static::lazy_static;
use rocksdb::Options;
use rocksdb::WriteBatch;
use rocksdb::DB;
use serde::Deserialize;
use serde::Serialize;
use sugars::hmap;
use tokio::sync::mpsc;

use super::rocks_cf::RocksCf;
use super::rocks_config::CacheSetting;
use super::rocks_config::DbConfig;
use super::rocks_db::create_new_backup;
use super::rocks_db::create_or_open_db;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
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
use crate::ext::named_spawn;
use crate::ext::OptionExt;
use crate::log_and_err;
use crate::utils::GIGABYTE;

cfg_if::cfg_if! {
    if #[cfg(feature = "metrics")] {
        use std::collections::HashMap;
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

/// State handler for our RocksDB storage, separating "tables" by column families.
///
/// With data separated by column families, writing and reading should be done via the `RocksCf` fields,
/// while operations that include the whole database (e.g. backup) should refer to the inner `DB` directly.
#[derive(Clone)]
pub struct RocksStorageState {
    pub db: Arc<DB>,
    pub db_path: PathBuf,
    pub accounts: RocksCf<AddressRocksdb, AccountRocksdb>,
    pub accounts_history: RocksCf<(AddressRocksdb, BlockNumberRocksdb), AccountRocksdb>,
    pub account_slots: RocksCf<(AddressRocksdb, SlotIndexRocksdb), SlotValueRocksdb>,
    pub account_slots_history: RocksCf<(AddressRocksdb, SlotIndexRocksdb, BlockNumberRocksdb), SlotValueRocksdb>,
    pub transactions: RocksCf<HashRocksdb, BlockNumberRocksdb>,
    pub blocks_by_number: RocksCf<BlockNumberRocksdb, BlockRocksdb>,
    pub blocks_by_hash: RocksCf<HashRocksdb, BlockNumberRocksdb>,
    pub logs: RocksCf<(HashRocksdb, IndexRocksdb), BlockNumberRocksdb>,
    pub backup_trigger: Arc<mpsc::Sender<()>>,
    /// Last collected stats for a histogram
    #[cfg(feature = "metrics")]
    pub prev_stats: Arc<Mutex<HashMap<HistogramInt, (Sum, Count)>>>,
    /// Options passed at DB creation, stored for metrics
    ///
    /// a newly created `rocksdb::Options` object is unique, with an underlying pointer identifier inside of it, and is used to access
    /// the DB metrics, `Options` can be cloned, but two equal `Options` might not retrieve the same metrics
    #[cfg(feature = "metrics")]
    pub db_options: Options,
}

impl RocksStorageState {
    pub fn new(path: impl AsRef<Path>) -> Self {
        let db_path = path.as_ref().to_path_buf();
        let (backup_trigger_tx, backup_trigger_rx) = mpsc::channel::<()>(1);

        // settings for each Column Family to be created
        let cf_options_iter = CF_OPTIONS_MAP.iter().map(|(name, opts)| (*name, opts.clone()));

        // options for the "default" column family (unused)
        let db_options = DbConfig::Default.to_options(CacheSetting::Disabled);

        let db = create_or_open_db(&db_path, &db_options, cf_options_iter).unwrap();

        //XXX TODO while repair/restore from backup, make sure to sync online and only when its in sync with other nodes, receive requests
        let state = Self {
            db_path,
            accounts: new_cf(&db, "accounts"),
            accounts_history: new_cf(&db, "accounts_history"),
            account_slots: new_cf(&db, "account_slots"),
            account_slots_history: new_cf(&db, "account_slots_history"),
            transactions: new_cf(&db, "transactions"),
            blocks_by_number: new_cf(&db, "blocks_by_number"),
            blocks_by_hash: new_cf(&db, "blocks_by_hash"), //XXX this is not needed we can afford to have blocks_by_hash pointing into blocks_by_number
            logs: new_cf(&db, "logs"),
            backup_trigger: Arc::new(backup_trigger_tx),
            #[cfg(feature = "metrics")]
            prev_stats: Default::default(),
            #[cfg(feature = "metrics")]
            db_options,
            db,
        };

        state.listen_for_backup_trigger(backup_trigger_rx).unwrap();

        state
    }

    fn listen_for_backup_trigger(&self, mut rx: mpsc::Receiver<()>) -> anyhow::Result<()> {
        tracing::info!("starting rocksdb backup trigger listener");

        let db = Arc::clone(&self.db);
        named_spawn("storage::listen_backup_trigger", async move {
            while rx.recv().await.is_some() {
                create_new_backup(&db).expect("failed to backup DB");
            }
        });

        Ok(())
    }

    pub fn preload_block_number(&self) -> anyhow::Result<AtomicU64> {
        let block_number = self.blocks_by_number.last().map(|(num, _)| num).unwrap_or_default();
        tracing::info!(number = %block_number, "preloaded block_number");
        Ok((u64::from(block_number)).into())
    }

    pub async fn reset_at(&self, block_number: BlockNumberRocksdb) -> anyhow::Result<()> {
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
                    latest_slots.insert((address, idx), (block.clone(), value.clone()));
                }
            } else {
                latest_slots.insert((address, idx), (block.clone(), value.clone()));
            }
        }
        for ((address, block), account) in self.accounts_history.iter_start() {
            if block > block_number {
                self.accounts_history.delete(&(address, block)).unwrap();
            } else if let Some((bnum, _)) = latest_accounts.get(&address) {
                if bnum < &block {
                    latest_accounts.insert(address, (block.clone(), account)).unwrap();
                }
            } else {
                latest_accounts.insert(address, (block.clone(), account));
            }
        }

        // write new current state
        let mut batch = WriteBatch::default();
        self.accounts.prepare_batch_insertion(
            latest_accounts.into_iter().map(|(address, (_, account))| (address, account)).collect(),
            &mut batch,
        );
        self.account_slots.prepare_batch_insertion(
            latest_slots.into_iter().map(|((address, idx), (_, value))| ((address, idx), value)).collect(),
            &mut batch,
        );
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
    pub fn update_state_with_execution_changes(&self, changes: &[ExecutionAccountChanges], block_number: BlockNumber, batch: &mut WriteBatch) {
        let accounts = self.accounts.clone();
        let accounts_history = self.accounts_history.clone();
        let account_slots = self.account_slots.clone();
        let account_slots_history = self.account_slots_history.clone();

        let mut account_changes = Vec::new();
        let mut account_history_changes = Vec::new();

        for change in changes {
            if change.is_changed() {
                let address: AddressRocksdb = change.address.into();
                let mut account_info_entry = accounts.entry_or_insert_with(address, AccountRocksdb::default);

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
        self.logs
            .iter_start()
            .skip_while(|(_, log_block_number)| log_block_number < &filter.from_block.into())
            .take_while(|(_, log_block_number)| match filter.to_block {
                Some(to_block) => log_block_number <= &to_block.into(),
                None => true,
            })
            .map(|((tx_hash, _), _)| match self.read_transaction(&tx_hash.into()) {
                Ok(Some(tx)) => Ok(tx.logs),
                Ok(None) => Err(anyhow!("the transaction the log was supposed to be in was not found")).with_context(|| format!("tx_hash = {:?}", tx_hash)),
                Err(err) => Err(err),
            })
            .flatten_ok()
            .filter_map(|log_res| match log_res {
                Ok(log) =>
                    if filter.matches(&log) {
                        Some(Ok(log))
                    } else {
                        None
                    },
                err => Some(err),
            })
            .collect()
    }

    pub fn read_slot(&self, address: &Address, index: &SlotIndex, point_in_time: &StoragePointInTime) -> Option<Slot> {
        if address.is_coinbase() {
            //XXX temporary, we will reload the database later without it
            return None;
        }

        match point_in_time {
            StoragePointInTime::Present => self.account_slots.get(&((*address).into(), (*index).into())).map(|account_slot_value| Slot {
                index: *index,
                value: account_slot_value.clone().into(),
            }),
            StoragePointInTime::Past(number) => {
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

    pub fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> Option<Account> {
        if address.is_coinbase() || address.is_zero() {
            //XXX temporary, we will reload the database later without it
            return None;
        }

        match point_in_time {
            StoragePointInTime::Present => match self.accounts.get(&((*address).into())) {
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
            StoragePointInTime::Past(block_number) => {
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

    pub fn read_block(&self, selection: &BlockSelection) -> Option<Block> {
        tracing::debug!(?selection, "reading block");

        let block = match selection {
            BlockSelection::Latest => self.blocks_by_number.iter_end().next().map(|(_, block)| block),
            BlockSelection::Earliest => self.blocks_by_number.iter_start().next().map(|(_, block)| block),
            BlockSelection::Number(number) => self.blocks_by_number.get(&(*number).into()),
            BlockSelection::Hash(hash) => {
                let block_number = self.blocks_by_hash.get(&(*hash).into()).unwrap_or_default();
                self.blocks_by_number.get(&block_number)
            }
        };
        match block {
            Some(block) => {
                tracing::trace!(?selection, ?block, "block found");
                Some(block.into())
            }
            None => None,
        }
    }

    /// Writes accounts to state (does not write to account history)
    #[allow(dead_code)]
    fn write_accounts(&self, accounts: Vec<Account>) {
        let accounts = accounts.into_iter().map(Into::into).collect_vec();

        let mut batch = WriteBatch::default();
        self.accounts.prepare_batch_insertion(accounts, &mut batch);
        self.accounts.db.write(batch).unwrap();
    }

    /// Writes slots to state (does not write to slot history)
    #[allow(dead_code)]
    fn write_slots(&self, slots: Vec<(Address, Slot)>) {
        let slots = slots
            .into_iter()
            .map(|(address, slot)| ((address.into(), slot.index.into()), slot.value.into()))
            .collect_vec();

        let mut batch = WriteBatch::default();
        self.account_slots.prepare_batch_insertion(slots, &mut batch);
        self.account_slots.db.write(batch).unwrap();
    }

    /// Write to all DBs in a batch
    pub fn write_batch(&self, batch: WriteBatch) -> anyhow::Result<()> {
        let batch_len = batch.len();
        let result = self.db.write(batch);

        if let Err(err) = &result {
            tracing::error!(?err, batch_len, "failed to write batch to DB");
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

fn new_cf<K, V>(db: &Arc<DB>, column_family: &str) -> RocksCf<K, V>
where
    K: Serialize + for<'de> Deserialize<'de> + std::hash::Hash + Eq,
    V: Serialize + for<'de> Deserialize<'de> + Clone,
{
    let options = CF_OPTIONS_MAP
        .get(&column_family)
        .unwrap_or_else(|| panic!("column_family `{column_family}` given to `new_cf` not found in options map"));
    RocksCf::new_cf(Arc::clone(db), column_family, options.clone())
}
