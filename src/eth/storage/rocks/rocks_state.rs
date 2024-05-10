use core::fmt;
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use anyhow::anyhow;
use futures::future::join_all;
use itertools::Itertools;
use num_traits::cast::ToPrimitive;
use tokio::sync::mpsc;
use tokio::task;
use tokio::task::JoinHandle;
use tracing::info;
use tracing::warn;

use super::rocks_db::DbConfig;
use super::rocks_db::RocksDb;
use super::types::AccountRocksdb;
use super::types::AddressRocksdb;
use super::types::BlockNumberRocksdb;
use super::types::BlockRocksdb;
use super::types::HashRocksdb;
use super::types::IndexRocksdb;
use super::types::SlotIndexRocksdb;
use super::types::SlotValueRocksdb;
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
use crate::ext::OptionExt;
use crate::log_and_err;

pub struct RocksStorageState {
    pub accounts: Arc<RocksDb<AddressRocksdb, AccountRocksdb>>,
    pub accounts_history: Arc<RocksDb<(AddressRocksdb, BlockNumberRocksdb), AccountRocksdb>>,
    pub account_slots: Arc<RocksDb<(AddressRocksdb, SlotIndexRocksdb), SlotValueRocksdb>>,
    pub account_slots_history: Arc<RocksDb<(AddressRocksdb, SlotIndexRocksdb, BlockNumberRocksdb), SlotValueRocksdb>>,
    pub transactions: Arc<RocksDb<HashRocksdb, BlockNumberRocksdb>>,
    pub blocks_by_number: Arc<RocksDb<BlockNumberRocksdb, BlockRocksdb>>,
    pub blocks_by_hash: Arc<RocksDb<HashRocksdb, BlockNumberRocksdb>>,
    pub logs: Arc<RocksDb<(HashRocksdb, IndexRocksdb), BlockNumberRocksdb>>,
    pub backup_trigger: Arc<mpsc::Sender<()>>,
}

impl Default for RocksStorageState {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel::<()>(1);

        //XXX TODO while repair/restore from backup, make sure to sync online and only when its in sync with other nodes, receive requests
        let state = Self {
            accounts: RocksDb::new("./data/accounts.rocksdb", DbConfig::Default).unwrap(),
            accounts_history: RocksDb::new("./data/accounts_history.rocksdb", DbConfig::FastWriteSST).unwrap(),
            account_slots: RocksDb::new("./data/account_slots.rocksdb", DbConfig::Default).unwrap(),
            account_slots_history: RocksDb::new("./data/account_slots_history.rocksdb", DbConfig::FastWriteSST).unwrap(),
            transactions: RocksDb::new("./data/transactions.rocksdb", DbConfig::LargeSSTFiles).unwrap(),
            blocks_by_number: RocksDb::new("./data/blocks_by_number.rocksdb", DbConfig::LargeSSTFiles).unwrap(),
            blocks_by_hash: RocksDb::new("./data/blocks_by_hash.rocksdb", DbConfig::LargeSSTFiles).unwrap(), //XXX this is not needed we can afford to have blocks_by_hash pointing into blocks_by_number
            logs: RocksDb::new("./data/logs.rocksdb", DbConfig::LargeSSTFiles).unwrap(),
            backup_trigger: Arc::new(tx),
        };

        state.listen_for_backup_trigger(rx).unwrap();

        state
    }
}

impl RocksStorageState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn listen_for_backup_trigger(&self, rx: mpsc::Receiver<()>) -> anyhow::Result<()> {
        tracing::info!("starting backup trigger listener");
        let accounts = Arc::<RocksDb<AddressRocksdb, AccountRocksdb>>::clone(&self.accounts);
        let accounts_history = Arc::<RocksDb<(AddressRocksdb, BlockNumberRocksdb), AccountRocksdb>>::clone(&self.accounts_history);
        let account_slots = Arc::<RocksDb<(AddressRocksdb, SlotIndexRocksdb), SlotValueRocksdb>>::clone(&self.account_slots);
        let account_slots_history =
            Arc::<RocksDb<(AddressRocksdb, SlotIndexRocksdb, BlockNumberRocksdb), SlotValueRocksdb>>::clone(&self.account_slots_history);
        let blocks_by_hash = Arc::<RocksDb<HashRocksdb, BlockNumberRocksdb>>::clone(&self.blocks_by_hash);
        let blocks_by_number = Arc::<RocksDb<BlockNumberRocksdb, BlockRocksdb>>::clone(&self.blocks_by_number);
        let transactions = Arc::<RocksDb<HashRocksdb, BlockNumberRocksdb>>::clone(&self.transactions);
        let logs = Arc::<RocksDb<(HashRocksdb, IndexRocksdb), BlockNumberRocksdb>>::clone(&self.logs);

        tokio::spawn(async move {
            let mut rx = rx;
            while rx.recv().await.is_some() {
                accounts.backup().unwrap();
                accounts_history.backup().unwrap();
                account_slots.backup().unwrap();
                account_slots_history.backup().unwrap();
                transactions.backup().unwrap();
                blocks_by_number.backup().unwrap();
                blocks_by_hash.backup().unwrap();
                logs.backup().unwrap();
            }
        });

        Ok(())
    }

    pub fn preload_block_number(&self) -> anyhow::Result<AtomicU64> {
        let account_block_number = self.accounts.get_current_block_number();

        Ok((account_block_number.to_u64().unwrap_or(0u64)).into())
    }

    pub async fn sync_data(&self) -> anyhow::Result<()> {
        tracing::info!("starting sync_data");
        tracing::info!("account_block_number {:?}", self.accounts.get_current_block_number());
        tracing::info!("slots_block_number {:?}", self.account_slots.get_current_block_number());
        tracing::info!("slots_history_block_number {:?}", self.account_slots_history.get_index_block_number());
        tracing::info!("accounts_history_block_number {:?}", self.accounts_history.get_index_block_number());
        tracing::info!("logs_block_number {:?}", self.logs.get_index_block_number());
        tracing::info!("transactions_block_number {:?}", self.transactions.get_index_block_number());

        if let Some((last_block_number, _)) = self.blocks_by_number.last() {
            tracing::info!("last_block_number {:?}", last_block_number);
            if self.accounts.get_current_block_number() != self.account_slots.get_current_block_number() {
                warn!(
                    "block numbers are not in sync {:?} {:?} {:?} {:?} {:?} {:?}",
                    self.accounts.get_current_block_number(),
                    self.account_slots.get_current_block_number(),
                    self.account_slots_history.get_index_block_number(),
                    self.accounts_history.get_index_block_number(),
                    self.logs.get_index_block_number(),
                    self.transactions.get_index_block_number(),
                );
                let mut min_block_number = std::cmp::min(
                    std::cmp::min(
                        std::cmp::min(self.accounts.get_current_block_number(), self.account_slots.get_current_block_number()),
                        std::cmp::min(
                            self.account_slots_history.get_index_block_number(),
                            self.accounts_history.get_index_block_number(),
                        ),
                    ),
                    std::cmp::min(self.logs.get_index_block_number(), self.transactions.get_index_block_number()),
                );

                let last_secure_block_number = last_block_number.inner_value().as_u64() - 5000;
                if last_secure_block_number > min_block_number {
                    self.accounts.restore().unwrap();
                    tracing::warn!("accounts restored");
                    self.accounts_history.restore().unwrap();
                    tracing::warn!("accounts_history restored");
                    self.account_slots.restore().unwrap();
                    tracing::warn!("account_slots restored");
                    self.account_slots_history.restore().unwrap();
                    tracing::warn!("account_slots_history restored");
                    self.transactions.restore().unwrap();
                    tracing::warn!("transactions restored");
                    self.blocks_by_number.restore().unwrap();
                    tracing::warn!("blocks_by_number restored");
                    self.blocks_by_hash.restore().unwrap();
                    tracing::warn!("blocks_by_hash restored");
                    self.logs.restore().unwrap();
                    tracing::warn!("logs restored");

                    min_block_number = std::cmp::min(
                        std::cmp::min(
                            std::cmp::min(self.accounts.get_current_block_number(), self.account_slots.get_current_block_number()),
                            std::cmp::min(
                                self.account_slots_history.get_index_block_number(),
                                self.accounts_history.get_index_block_number(),
                            ),
                        ),
                        std::cmp::min(self.logs.get_index_block_number(), self.transactions.get_index_block_number()),
                    );
                }
                self.reset_at(BlockNumber::from(min_block_number)).await?;
            }
        }

        tracing::info!("data is in sync");

        Ok(())
    }

    pub async fn reset_at(&self, block_number: BlockNumber) -> anyhow::Result<()> {
        let tasks = vec![
            {
                let self_blocks_by_hash_clone = Arc::clone(&self.blocks_by_hash);
                let block_number_clone = block_number;
                task::spawn_blocking(move || {
                    for (block_num, block_hash_vec) in self_blocks_by_hash_clone.indexed_iter_end() {
                        if block_num <= block_number_clone.as_u64() {
                            break;
                        }
                        for block_hash in block_hash_vec {
                            self_blocks_by_hash_clone.delete(&block_hash).unwrap();
                        }
                        self_blocks_by_hash_clone.delete_index(block_num).unwrap();
                    }

                    info!(
                        "Deleted blocks by hash above block number {}. This ensures synchronization with the lowest block height across nodes.",
                        block_number_clone
                    );
                })
            },
            {
                let self_blocks_by_number_clone = Arc::clone(&self.blocks_by_number);
                let block_number_clone = block_number;
                task::spawn_blocking(move || {
                    let blocks_by_number = self_blocks_by_number_clone.iter_end();
                    for (num, _) in blocks_by_number {
                        if num <= block_number_clone.into() {
                            break;
                        }
                        self_blocks_by_number_clone.delete(&num).unwrap();
                    }
                    info!(
                        "Deleted blocks by number above block number {}. Helps in reverting to a common state prior to a network fork or error.",
                        block_number_clone
                    );
                })
            },
            {
                let self_transactions_clone = Arc::clone(&self.transactions);
                let block_number_clone = block_number;
                task::spawn_blocking(move || {
                    let transactions = self_transactions_clone.indexed_iter_end();
                    for (index_block_number, hash_vec) in transactions {
                        if index_block_number <= block_number_clone.as_u64() {
                            break;
                        }
                        for hash in hash_vec {
                            self_transactions_clone.delete(&hash).unwrap();
                        }
                        self_transactions_clone.delete_index(index_block_number).unwrap();
                    }
                    info!(
                        "Cleared transactions above block number {}. Necessary to remove transactions not confirmed in the finalized blockchain state.",
                        block_number_clone
                    );
                })
            },
            {
                let self_logs_clone = Arc::clone(&self.logs);
                let block_number_clone = block_number;
                task::spawn_blocking(move || {
                    let logs = self_logs_clone.indexed_iter_end();
                    for (index_block_number, logs_vec) in logs {
                        if index_block_number <= block_number_clone.as_u64() {
                            break;
                        }
                        for (hash, index) in logs_vec {
                            self_logs_clone.delete(&(hash, index)).unwrap();
                        }
                        self_logs_clone.delete_index(index_block_number).unwrap();
                    }
                    info!(
                        "Removed logs above block number {}. Ensures log consistency with the blockchain's current confirmed state.",
                        block_number_clone
                    );
                })
            },
            {
                let self_accounts_history_clone = Arc::clone(&self.accounts_history);
                let block_number_clone = block_number;
                task::spawn_blocking(move || {
                    let accounts_history = self_accounts_history_clone.indexed_iter_end();
                    for (index_block_number, accounts_history_vec) in accounts_history {
                        if index_block_number <= block_number_clone.as_u64() {
                            break;
                        }
                        for (address, historic_block_number) in accounts_history_vec {
                            self_accounts_history_clone.delete(&(address, historic_block_number)).unwrap();
                        }
                        self_accounts_history_clone.delete_index(index_block_number).unwrap();
                    }
                    info!(
                        "Deleted account history records above block number {}. Important for maintaining historical accuracy in account state across nodes.",
                        block_number_clone
                    );
                })
            },
            {
                let self_account_slots_history_clone = Arc::clone(&self.account_slots_history);
                let block_number_clone = block_number;
                task::spawn_blocking(move || {
                    let account_slots_history = self_account_slots_history_clone.indexed_iter_end();
                    for (index_block_number, account_slots_history_vec) in account_slots_history {
                        if index_block_number <= block_number_clone.as_u64() {
                            break;
                        }
                        for (address, slot_index, historic_block_number) in account_slots_history_vec {
                            self_account_slots_history_clone.delete(&(address, slot_index, historic_block_number)).unwrap();
                        }
                        self_account_slots_history_clone.delete_index(index_block_number).unwrap();
                    }
                    info!(
                        "Cleared account slot history above block number {}. Vital for synchronizing account slot states after discrepancies.",
                        block_number_clone
                    );
                })
            },
        ];

        // Wait for all tasks to complete using join_all
        let _ = join_all(tasks).await;

        // Clear current states
        let _ = self.accounts.clear();
        let _ = self.account_slots.clear();

        // Spawn task for handling accounts
        let accounts_task = task::spawn_blocking({
            let self_accounts_history_clone = Arc::clone(&self.accounts_history);
            let self_accounts_clone = Arc::clone(&self.accounts);
            let block_number_clone = block_number;
            move || {
                let mut latest_accounts: HashMap<AddressRocksdb, (BlockNumberRocksdb, AccountRocksdb)> = std::collections::HashMap::new();
                let account_histories = self_accounts_history_clone.iter_start();
                for ((address, historic_block_number), account_info) in account_histories {
                    if let Some((existing_block_number, _)) = latest_accounts.get(&address) {
                        if existing_block_number < &historic_block_number {
                            latest_accounts.insert(address, (historic_block_number, account_info));
                        }
                    } else {
                        latest_accounts.insert(address, (historic_block_number, account_info));
                    }
                }

                let accounts_temp_vec = latest_accounts
                    .into_iter()
                    .map(|(address, (_, account_info))| (address, account_info))
                    .collect::<Vec<_>>();
                self_accounts_clone.insert_batch(accounts_temp_vec, Some(block_number_clone.into()));
                info!("Accounts updated up to block number {}", block_number_clone);
            }
        });

        // Spawn task for handling slots
        let slots_task = task::spawn_blocking({
            let self_account_slots_history_clone = Arc::clone(&self.account_slots_history);
            let self_account_slots_clone = Arc::clone(&self.account_slots);
            let block_number_clone = block_number;
            move || {
                let mut latest_slots: HashMap<(AddressRocksdb, SlotIndexRocksdb), (BlockNumberRocksdb, SlotValueRocksdb)> = std::collections::HashMap::new();
                let slot_histories = self_account_slots_history_clone.iter_start();
                for ((address, slot_index, historic_block_number), slot_value) in slot_histories {
                    let slot_key = (address, slot_index);
                    if let Some((existing_block_number, _)) = latest_slots.get(&slot_key) {
                        if existing_block_number < &historic_block_number {
                            latest_slots.insert(slot_key, (historic_block_number, slot_value));
                        }
                    } else {
                        latest_slots.insert(slot_key, (historic_block_number, slot_value));
                    }
                }

                let slots_temp_vec = latest_slots
                    .into_iter()
                    .map(|((address, slot_index), (_, slot_value))| ((address, slot_index), slot_value))
                    .collect::<Vec<_>>();
                self_account_slots_clone.insert_batch(slots_temp_vec, Some(block_number_clone.into()));
                info!("Slots updated up to block number {}", block_number_clone);
            }
        });

        let _ = join_all(vec![accounts_task, slots_task]).await;

        info!(
            "All reset tasks have been completed or encountered errors. The system is now aligned to block number {}.",
            block_number
        );

        Ok(())
    }

    /// Updates the in-memory state with changes from transaction execution
    pub fn update_state_with_execution_changes(
        &self,
        changes: &[ExecutionAccountChanges],
        block_number: BlockNumber,
    ) -> Result<Vec<JoinHandle<()>>, sqlx::Error> {
        // Directly capture the fields needed by each future from `self`
        let accounts = Arc::clone(&self.accounts);
        let accounts_history = Arc::clone(&self.accounts_history);
        let account_slots = Arc::clone(&self.account_slots);
        let account_slots_history = Arc::clone(&self.account_slots_history);

        let changes_clone_for_accounts = changes.to_vec(); // Clone changes for accounts future
        let changes_clone_for_slots = changes.to_vec(); // Clone changes for slots future

        let mut account_changes = Vec::new();
        let mut account_history_changes = Vec::new();

        let account_changes_future = tokio::task::spawn_blocking(move || {
            for change in changes_clone_for_accounts {
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

            accounts.insert_batch(account_changes, Some(block_number.into()));
            accounts_history.insert_batch_indexed(account_history_changes, block_number.into());
        });

        let mut slot_changes = Vec::new();
        let mut slot_history_changes = Vec::new();

        let slot_changes_future = tokio::task::spawn_blocking(move || {
            for change in changes_clone_for_slots {
                let address: AddressRocksdb = change.address.into();
                for (slot_index, slot_change) in change.slots.clone() {
                    if let Some(slot) = slot_change.take_modified() {
                        slot_changes.push(((address, slot_index.into()), slot.value.into()));
                        slot_history_changes.push(((address, slot_index.into(), block_number.into()), slot.value.into()));
                    }
                }
            }
            account_slots.insert_batch(slot_changes, Some(block_number.into()));
            account_slots_history.insert_batch_indexed(slot_history_changes, block_number.into());
        });

        Ok(vec![account_changes_future, slot_changes_future])
    }

    pub fn read_transaction(&self, tx_hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        match self.transactions.get(&(*tx_hash).into()) {
            Some(transaction) => match self.blocks_by_number.get(&transaction) {
                Some(block) => {
                    tracing::trace!(%tx_hash, "transaction found");
                    match block.transactions.into_iter().find(|tx| &Hash::from(tx.input.hash) == tx_hash) {
                        Some(tx) => Ok(Some(tx.into())),
                        None => log_and_err!("transaction was not found in block"),
                    }
                }
                None => {
                    log_and_err!("the block that the transaction was supposed to be in was not found")
                }
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
                Ok(None) => Err(anyhow!("the transaction the log was supposed to be in was not found")),
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
    pub fn write_accounts(&self, accounts: Vec<Account>, block_number: BlockNumber) {
        let mut account_batch = vec![];
        for account in accounts {
            account_batch.push(account.into());
        }

        self.accounts.insert_batch(account_batch, Some(block_number.into()));
    }

    /// Writes slots to state (does not write to slot history)
    pub fn write_slots(&self, slots: Vec<(Address, Slot)>, block_number: BlockNumber) {
        let mut slot_batch = vec![];

        for (address, slot) in slots {
            slot_batch.push(((address.into(), slot.index.into()), slot.value.into()));
        }
        self.account_slots.insert_batch(slot_batch, Some(block_number.into()));
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

    #[cfg(feature = "metrics")]
    pub fn export_metrics(&self) {
        self.account_slots.export_metrics();
        self.account_slots_history.export_metrics();

        self.accounts.export_metrics();
        self.accounts_history.export_metrics();

        self.blocks_by_hash.export_metrics();
        self.blocks_by_number.export_metrics();
    }
}

impl fmt::Debug for RocksStorageState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RocksDb").field("db", &"Arc<DB>").finish()
    }
}
