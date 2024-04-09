use core::fmt;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use async_trait::async_trait;
use futures::future::join_all;
use itertools::Itertools;
use num_traits::cast::ToPrimitive;
use revm::primitives::KECCAK_EMPTY;
use tokio::task::JoinHandle;
use tracing::warn;

use super::rocks_db::DbConfig;
use super::rocks_db::RocksDb;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CodeHash;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::ExecutionConflicts;
use crate::eth::primitives::ExecutionConflictsBuilder;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotSample;
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::Wei;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::StorageError;
use crate::log_and_err;

#[derive(Debug)]
pub struct RocksPermanentStorage {
    state: RocksStorageState, //XXX TODO remove RwLock when rocksdb is implemented everywhere
    block_number: AtomicU64,
}

impl RocksPermanentStorage {
    pub fn new() -> anyhow::Result<Self> {
        tracing::info!("starting rocksdb storage");

        let state = RocksStorageState::new();
        state.sync_data()?;
        let block_number = state.preload_block_number()?;
        Ok(Self { state, block_number })
    }

    // -------------------------------------------------------------------------
    // State methods
    // -------------------------------------------------------------------------

    /// Clears in-memory state.
    pub fn clear(&self) {
        let _ = self.state.accounts.clear();
        let _ = self.state.accounts_history.clear();
        let _ = self.state.account_slots.clear();
        let _ = self.state.account_slots_history.clear();

        self.state.transactions.clear().unwrap();
        self.state.blocks_by_hash.clear().unwrap();
        self.state.blocks_by_number.clear().unwrap();
        self.state.logs.clear().unwrap();
    }

    fn check_conflicts(state: &RocksStorageState, account_changes: &[ExecutionAccountChanges]) -> Option<ExecutionConflicts> {
        let mut conflicts = ExecutionConflictsBuilder::default();

        for change in account_changes {
            let address = &change.address;

            if let Some(account) = state.accounts.get(address) {
                // check account info conflicts
                if let Some(original_nonce) = change.nonce.take_original_ref() {
                    let account_nonce = &account.nonce;
                    if original_nonce != account_nonce {
                        conflicts.add_nonce(address.clone(), account_nonce.clone(), original_nonce.clone());
                    }
                }
                if let Some(original_balance) = change.balance.take_original_ref() {
                    let account_balance = &account.balance;
                    if original_balance != account_balance {
                        conflicts.add_balance(address.clone(), account_balance.clone(), original_balance.clone());
                    }
                }
                // check slots conflicts
                for (slot_index, slot_change) in &change.slots {
                    if let Some(value) = state.account_slots.get(&(address.clone(), slot_index.clone())) {
                        if let Some(original_slot) = slot_change.take_original_ref() {
                            let account_slot_value = value.clone();
                            if original_slot.value != account_slot_value {
                                conflicts.add_slot(address.clone(), slot_index.clone(), account_slot_value, original_slot.value.clone());
                            }
                        }
                    }
                }
            }
        }
        conflicts.build()
    }
}

#[async_trait]
impl PermanentStorage for RocksPermanentStorage {
    async fn allocate_evm_thread_resources(&self) -> anyhow::Result<()> {
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Block number operations
    // -------------------------------------------------------------------------

    async fn read_mined_block_number(&self) -> anyhow::Result<BlockNumber> {
        Ok(self.block_number.load(Ordering::SeqCst).into())
    }

    async fn increment_block_number(&self) -> anyhow::Result<BlockNumber> {
        let next = self.block_number.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(next.into())
    }

    async fn set_mined_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        self.block_number.store(number.as_u64(), Ordering::SeqCst);
        Ok(())
    }

    // -------------------------------------------------------------------------
    // State operations
    // ------------------------------------------------------------------------

    async fn maybe_read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>> {
        let account = match point_in_time {
            StoragePointInTime::Present => match self.state.accounts.get(address) {
                Some(inner_account) => {
                    let account = inner_account.to_account(address).await;
                    tracing::trace!(%address, ?account, "account found");
                    Some(account)
                }

                None => {
                    tracing::trace!(%address, "account not found");
                    None
                }
            },
            StoragePointInTime::Past(block_number) => {
                if let Some(((addr, _), account_info)) = self
                    .state
                    .accounts_history
                    .iter_from((address.clone(), *block_number), rocksdb::Direction::Reverse)
                    .next()
                {
                    if address == &addr {
                        return Ok(Some(account_info.to_account(address).await));
                    }
                }
                return Ok(None);
            }
        };
        Ok(account)
    }

    async fn maybe_read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        tracing::debug!(%address, %slot_index, ?point_in_time, "reading slot");
        self.state.get_slot_at_point(address, slot_index, point_in_time)
    }

    async fn read_block(&self, selection: &BlockSelection) -> anyhow::Result<Option<Block>> {
        // TODO read from pg if not in memory
        tracing::debug!(?selection, "reading block");

        let block = match selection {
            BlockSelection::Latest => self.state.blocks_by_number.iter_end().next().map(|(_, block)| block),
            BlockSelection::Earliest => self.state.blocks_by_number.iter_start().next().map(|(_, block)| block),
            BlockSelection::Number(number) => self.state.blocks_by_number.get(number),
            BlockSelection::Hash(hash) => {
                let block_number = self.state.blocks_by_hash.get(hash).unwrap_or_default();
                self.state.blocks_by_number.get(&block_number)
            }
        };
        match block {
            Some(block) => {
                tracing::trace!(?selection, ?block, "block found");
                Ok(Some(block))
            }
            None => Ok(None),
        }
    }

    async fn read_mined_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        tracing::debug!(%hash, "reading transaction");
        self.state.read_transaction(hash)
    }

    async fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        tracing::debug!(?filter, "reading logs");
        self.state.read_logs(filter)
    }

    async fn save_block(&self, block: Block) -> anyhow::Result<(), StorageError> {
        // check conflicts before persisting any state changes
        let account_changes = block.compact_account_changes();
        if let Some(conflicts) = Self::check_conflicts(&self.state, &account_changes) {
            return Err(StorageError::Conflict(conflicts));
        }

        let mut futures = Vec::with_capacity(9);

        let mut txs_batch = vec![];
        let mut logs_batch = vec![];
        for transaction in block.transactions.clone() {
            txs_batch.push((transaction.input.hash.clone(), transaction.block_number));
            for log in transaction.logs {
                logs_batch.push(((transaction.input.hash.clone(), log.log_index), transaction.block_number));
            }
        }

        let txs_rocks = Arc::clone(&self.state.transactions);
        let logs_rocks = Arc::clone(&self.state.logs);
        futures.push(tokio::task::spawn_blocking(move || txs_rocks.insert_batch(txs_batch, None)));
        futures.push(tokio::task::spawn_blocking(move || logs_rocks.insert_batch(logs_batch, None)));

        // save block
        let number = *block.number();
        let hash = block.hash().clone();

        let blocks_by_number = Arc::clone(&self.state.blocks_by_number);
        let blocks_by_hash = Arc::clone(&self.state.blocks_by_hash);
        let mut block_without_changes = block.clone();
        for transaction in &mut block_without_changes.transactions {
            transaction.execution.changes = vec![];
        }
        let hash_clone = hash.clone();
        futures.push(tokio::task::spawn_blocking(move || blocks_by_number.insert(number, block_without_changes)));
        futures.push(tokio::task::spawn_blocking(move || blocks_by_hash.insert(hash_clone, number)));

        futures.append(
            &mut self
                .state
                .update_state_with_execution_changes(&account_changes, number)
                .context("failed to update state with execution changes")?,
        );

        join_all(futures).await;
        Ok(())
    }

    async fn after_commit_hook(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        tracing::debug!(?accounts, "saving initial accounts");

        for account in accounts {
            self.state.accounts.insert(
                account.address.clone(),
                AccountInfo {
                    balance: account.balance.clone(),
                    nonce: account.nonce.clone(),
                    bytecode: account.bytecode.clone(),
                    code_hash: account.code_hash.clone(),
                },
            );

            self.state.accounts_history.insert(
                (account.address.clone(), 0.into()),
                AccountInfo {
                    balance: account.balance.clone(),
                    nonce: account.nonce.clone(),
                    bytecode: account.bytecode.clone(),
                    code_hash: account.code_hash.clone(),
                },
            );
        }

        Ok(())
    }

    async fn reset_at(&self, block_number: BlockNumber) -> anyhow::Result<()> {
        // reset block number
        let block_number_u64: u64 = block_number.into();
        let _ = self.block_number.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
            if block_number_u64 <= current {
                Some(block_number_u64)
            } else {
                None
            }
        });

        self.state.reset_at(block_number)
    }

    async fn read_slots_sample(&self, _start: BlockNumber, _end: BlockNumber, _max_samples: u64, _seed: u64) -> anyhow::Result<Vec<SlotSample>> {
        todo!()
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct AccountInfo {
    pub balance: Wei,
    pub nonce: Nonce,
    pub bytecode: Option<Bytes>,
    pub code_hash: CodeHash,
}

impl AccountInfo {
    pub async fn to_account(&self, address: &Address) -> Account {
        Account {
            address: address.clone(),
            nonce: self.nonce.clone(),
            balance: self.balance.clone(),
            bytecode: self.bytecode.clone(),
            code_hash: self.code_hash.clone(),
        }
    }
}

pub struct RocksStorageState {
    pub accounts: Arc<RocksDb<Address, AccountInfo>>,
    pub accounts_history: Arc<RocksDb<(Address, BlockNumber), AccountInfo>>,
    pub account_slots: Arc<RocksDb<(Address, SlotIndex), SlotValue>>,
    pub account_slots_history: Arc<RocksDb<(Address, SlotIndex, BlockNumber), SlotValue>>,
    pub transactions: Arc<RocksDb<Hash, BlockNumber>>,
    pub blocks_by_number: Arc<RocksDb<BlockNumber, Block>>,
    pub blocks_by_hash: Arc<RocksDb<Hash, BlockNumber>>,
    pub logs: Arc<RocksDb<(Hash, Index), BlockNumber>>,
}

impl RocksStorageState {
    pub fn new() -> Self {
        Self {
            accounts: Arc::new(RocksDb::new("./data/accounts.rocksdb", DbConfig::Default).unwrap()),
            accounts_history: Arc::new(RocksDb::new("./data/accounts_history.rocksdb", DbConfig::LargeSSTFiles).unwrap()),
            account_slots: Arc::new(RocksDb::new("./data/account_slots.rocksdb", DbConfig::Default).unwrap()),
            account_slots_history: Arc::new(RocksDb::new("./data/account_slots_history.rocksdb", DbConfig::LargeSSTFiles).unwrap()),
            transactions: Arc::new(RocksDb::new("./data/transactions.rocksdb", DbConfig::LargeSSTFiles).unwrap()),
            blocks_by_number: Arc::new(RocksDb::new("./data/blocks_by_number.rocksdb", DbConfig::LargeSSTFiles).unwrap()),
            blocks_by_hash: Arc::new(RocksDb::new("./data/blocks_by_hash.rocksdb", DbConfig::LargeSSTFiles).unwrap()), //XXX this is not needed we can afford to have blocks_by_hash pointing into blocks_by_number
            logs: Arc::new(RocksDb::new("./data/logs.rocksdb", DbConfig::LargeSSTFiles).unwrap()),
        }
    }

    fn preload_block_number(&self) -> anyhow::Result<AtomicU64> {
        let account_block_number = self.accounts.get_current_block_number();

        Ok((account_block_number.to_u64().unwrap_or(0u64)).into())
    }

    pub fn sync_data(&self) -> anyhow::Result<()> {
        let account_block_number = self.accounts.get_current_block_number();
        let slots_block_number = self.account_slots.get_current_block_number();
        if account_block_number != slots_block_number {
            warn!("block numbers are not in sync");
            let min_block_number = std::cmp::min(account_block_number, slots_block_number);
            self.reset_at(BlockNumber::from(min_block_number))?;
        }

        Ok(())
    }

    fn reset_at(&self, block_number: BlockNumber) -> anyhow::Result<()> {
        // Remove blocks by hash that are greater than block_number
        let blocks_by_hash = self.blocks_by_hash.iter_start();
        for (block_hash, block_num) in blocks_by_hash {
            if block_num > block_number {
                self.blocks_by_hash.delete(&block_hash)?;
            }
        }

        // Remove blocks by number that are greater than block_number
        let blocks_by_number = self.blocks_by_number.iter_start();
        for (num, _) in blocks_by_number {
            if num > block_number {
                self.blocks_by_number.delete(&num)?;
            }
        }

        let transactions = self.transactions.iter_start();
        for (hash, tx_block_number) in transactions {
            if tx_block_number > block_number {
                self.transactions.delete(&hash)?;
            }
        }

        let logs = self.logs.iter_start();
        for (key, log_block_number) in logs {
            if log_block_number > block_number {
                self.logs.delete(&key)?;
            }
        }

        let accounts_historic = self.accounts_history.iter_start();
        for ((address, historic_block_number), _) in accounts_historic {
            if historic_block_number > block_number {
                self.accounts_history.delete(&(address, historic_block_number))?;
            }
        }

        let account_slots_historic = self.account_slots_history.iter_start();
        for ((address, slot_index, historic_block_number), _) in account_slots_historic {
            if historic_block_number > block_number {
                self.account_slots_history.delete(&(address, slot_index, historic_block_number))?;
            }
        }

        let _ = self.accounts.clear();
        let _ = self.account_slots.clear();

        // Use HashMaps to store only the latest entries for each account and slot
        let mut latest_accounts = std::collections::HashMap::new();
        let mut latest_slots = std::collections::HashMap::new();

        // Populate latest_accounts with the most recent account info up to block_number
        let account_histories = self.accounts_history.iter_start();
        for ((address, historic_block_number), account_info) in account_histories {
            if let Some((existing_block_number, _)) = latest_accounts.get(&address) {
                if *existing_block_number < historic_block_number {
                    latest_accounts.insert(address, (historic_block_number, account_info));
                }
            } else {
                latest_accounts.insert(address, (historic_block_number, account_info));
            }
        }

        // Insert the most recent account information from latest_accounts into the current state
        for (address, (_, account_info)) in latest_accounts {
            self.accounts.insert(address, account_info);
        }

        // Populate latest_slots with the most recent slot info up to block_number
        let slot_histories = self.account_slots_history.iter_start();
        for ((address, slot_index, historic_block_number), slot_value) in slot_histories {
            let slot_key = (address, slot_index);
            if let Some((existing_block_number, _)) = latest_slots.get(&slot_key) {
                if *existing_block_number < historic_block_number {
                    latest_slots.insert(slot_key, (historic_block_number, slot_value));
                }
            } else {
                latest_slots.insert(slot_key, (historic_block_number, slot_value));
            }
        }

        // Insert the most recent slot information from latest_slots into the current state
        for ((address, slot_index), (_, slot_value)) in latest_slots {
            self.account_slots.insert((address, slot_index), slot_value);
        }

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
                let address = change.address.clone();
                let mut account_info_entry = accounts.entry_or_insert_with(address.clone(), || AccountInfo {
                    balance: Wei::ZERO, // Initialize with default values
                    nonce: Nonce::ZERO,
                    bytecode: None,
                    code_hash: KECCAK_EMPTY.into(),
                });
                if let Some(nonce) = change.nonce.clone().take_modified() {
                    account_info_entry.nonce = nonce;
                }
                if let Some(balance) = change.balance.clone().take_modified() {
                    account_info_entry.balance = balance;
                }
                if let Some(bytecode) = change.bytecode.clone().take_modified() {
                    account_info_entry.bytecode = bytecode;
                }
                account_changes.push((address.clone(), account_info_entry.clone()));
                account_history_changes.push(((address.clone(), block_number), account_info_entry));
            }

            accounts.insert_batch(account_changes, Some(block_number.as_i64()));
            accounts_history.insert_batch(account_history_changes, None);
        });

        let mut slot_changes = Vec::new();
        let mut slot_history_changes = Vec::new();

        let slot_changes_future = tokio::task::spawn_blocking(move || {
            for change in changes_clone_for_slots {
                let address = change.address.clone();
                for (slot_index, slot_change) in change.slots.clone() {
                    if let Some(slot) = slot_change.take_modified() {
                        slot_changes.push(((address.clone(), slot_index.clone()), slot.value.clone()));
                        slot_history_changes.push(((address.clone(), slot_index, block_number), slot.value));
                    }
                }
            }
            account_slots.insert_batch(slot_changes, Some(block_number.as_i64()));
            account_slots_history.insert_batch(slot_history_changes, None);
        });

        Ok(vec![account_changes_future, slot_changes_future])
    }

    pub fn read_transaction(&self, tx_hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        match self.transactions.get(tx_hash) {
            Some(transaction) => match self.blocks_by_number.get(&transaction) {
                Some(block) => {
                    tracing::trace!(%tx_hash, "transaction found in memory");
                    match block.transactions.into_iter().find(|tx| &tx.input.hash == tx_hash) {
                        Some(tx) => Ok(Some(tx)),
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
            .skip_while(|(_, log_block_number)| log_block_number < &filter.from_block)
            .take_while(|(_, log_block_number)| match filter.to_block {
                Some(to_block) => log_block_number <= &to_block,
                None => true,
            })
            .map(|((tx_hash, _), _)| match self.read_transaction(&tx_hash) {
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

    pub fn get_slot_at_point(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        let slot = match point_in_time {
            StoragePointInTime::Present => self.account_slots.get(&(address.clone(), slot_index.clone())).map(|account_slot_value| Slot {
                index: slot_index.clone(),
                value: account_slot_value.clone(),
            }),
            StoragePointInTime::Past(number) => {
                if let Some(((addr, index, _), value)) = self
                    .account_slots_history
                    .iter_from((address.clone(), slot_index.clone(), *number), rocksdb::Direction::Reverse)
                    .next()
                {
                    if slot_index == &index && address == &addr {
                        return Ok(Some(Slot { index, value }));
                    }
                }
                return Ok(None);
            }
        };
        Ok(slot)
    }
}

impl fmt::Debug for RocksStorageState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RocksDb").field("db", &"Arc<DB>").finish()
    }
}
