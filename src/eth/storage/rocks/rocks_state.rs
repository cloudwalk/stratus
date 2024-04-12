use core::fmt;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use anyhow::anyhow;
use futures::future::join_all;
use itertools::Itertools;
use num_traits::cast::ToPrimitive;
use revm::primitives::KECCAK_EMPTY;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::warn;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CodeHash;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::Wei;
use crate::eth::storage::rocks_db::DbConfig;
use crate::eth::storage::rocks_db::RocksDb;
use crate::log_and_err;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct AccountInfo {
    pub balance: Wei,
    pub nonce: Nonce,
    pub bytecode: Option<Bytes>,
    pub code_hash: CodeHash,
}

impl AccountInfo {
    pub fn to_account(&self, address: &Address) -> Account {
        Account {
            address: address.clone(),
            nonce: self.nonce.clone(),
            balance: self.balance.clone(),
            bytecode: self.bytecode.clone(),
            code_hash: self.code_hash.clone(),
            static_slot_indexes: None,  // TODO: is it necessary for RocksDB?
            mapping_slot_indexes: None, // TODO: is it necessary for RocksDB?
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
    pub backup_trigger: Arc<mpsc::Sender<()>>,
}

impl RocksStorageState {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<()>(1);

        let state = Self {
            accounts: Arc::new(RocksDb::new("./data/accounts.rocksdb", DbConfig::Default).unwrap()),
            accounts_history: Arc::new(RocksDb::new("./data/accounts_history.rocksdb", DbConfig::LargeSSTFiles).unwrap()),
            account_slots: Arc::new(RocksDb::new("./data/account_slots.rocksdb", DbConfig::Default).unwrap()),
            account_slots_history: Arc::new(RocksDb::new("./data/account_slots_history.rocksdb", DbConfig::LargeSSTFiles).unwrap()),
            transactions: Arc::new(RocksDb::new("./data/transactions.rocksdb", DbConfig::LargeSSTFiles).unwrap()),
            blocks_by_number: Arc::new(RocksDb::new("./data/blocks_by_number.rocksdb", DbConfig::LargeSSTFiles).unwrap()),
            blocks_by_hash: Arc::new(RocksDb::new("./data/blocks_by_hash.rocksdb", DbConfig::LargeSSTFiles).unwrap()), //XXX this is not needed we can afford to have blocks_by_hash pointing into blocks_by_number
            logs: Arc::new(RocksDb::new("./data/logs.rocksdb", DbConfig::LargeSSTFiles).unwrap()),
            backup_trigger: Arc::new(tx),
        };

        state.listen_for_backup_trigger(rx).unwrap();

        state
    }

    pub fn listen_for_backup_trigger(&self, rx: mpsc::Receiver<()>) -> anyhow::Result<()> {
        let accounts = Arc::<RocksDb<Address, AccountInfo>>::clone(&self.accounts);
        let accounts_history = Arc::<RocksDb<(Address, BlockNumber), AccountInfo>>::clone(&self.accounts_history);
        let account_slots = Arc::<RocksDb<(Address, SlotIndex), SlotValue>>::clone(&self.account_slots);
        let account_slots_history = Arc::<RocksDb<(Address, SlotIndex, BlockNumber), SlotValue>>::clone(&self.account_slots_history);
        let blocks_by_hash = Arc::<RocksDb<Hash, BlockNumber>>::clone(&self.blocks_by_hash);
        let blocks_by_number = Arc::<RocksDb<BlockNumber, Block>>::clone(&self.blocks_by_number);
        let transactions = Arc::<RocksDb<Hash, BlockNumber>>::clone(&self.transactions);
        let logs = Arc::<RocksDb<(Hash, Index), BlockNumber>>::clone(&self.logs);

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

    pub fn sync_data(&self) -> anyhow::Result<()> {
        let account_block_number = self.accounts.get_current_block_number();
        let slots_block_number = self.account_slots.get_current_block_number();
        if let Some((last_block_number, _)) = self.blocks_by_number.last() {
            if account_block_number != slots_block_number {
                warn!("block numbers are not in sync {:?} {:?}", account_block_number, slots_block_number);
                let min_block_number = std::cmp::min(account_block_number, slots_block_number);
                let last_secure_block_number = last_block_number.as_i64() - 1000;
                if last_secure_block_number > min_block_number {
                    panic!("block numbers is too far away from the last secure block number, please resync the data from the last secure block number");
                }
                self.reset_at(BlockNumber::from(min_block_number))?;
            }
        }

        Ok(())
    }

    pub fn reset_at(&self, block_number: BlockNumber) -> anyhow::Result<()> {
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
        let mut accounts_temp_vec = vec![];
        for (address, (_, account_info)) in latest_accounts {
            accounts_temp_vec.push((address, account_info));
        }

        self.accounts.insert_batch(accounts_temp_vec, Some(block_number.into()));

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
        let mut slots_temp_vec = vec![];
        for ((address, slot_index), (_, slot_value)) in latest_slots {
            slots_temp_vec.push(((address, slot_index), slot_value));
        }

        self.account_slots.insert_batch(slots_temp_vec, Some(block_number.into()));

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

            accounts.insert_batch(account_changes, Some(block_number.into()));
            //accounts_history.insert_batch(account_history_changes, None);
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
            account_slots.insert_batch(slot_changes, Some(block_number.into()));
          //  account_slots_history.insert_batch(slot_history_changes, None);
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

    pub fn read_slot(&self, address: &Address, index: &SlotIndex, point_in_time: &StoragePointInTime) -> Option<Slot> {
        match point_in_time {
            StoragePointInTime::Present => self.account_slots.get(&(address.clone(), index.clone())).map(|account_slot_value| Slot {
                index: index.clone(),
                value: account_slot_value.clone(),
            }),
            StoragePointInTime::Past(number) => {
                if let Some(((rocks_address, rocks_index, _), value)) = self
                    .account_slots_history
                    .iter_from((address.clone(), index.clone(), *number), rocksdb::Direction::Reverse)
                    .next()
                {
                    if index == &rocks_index && address == &rocks_address {
                        return Some(Slot { index: rocks_index, value });
                    }
                }
                None
            }
        }
    }

    pub fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> Option<Account> {
        match point_in_time {
            StoragePointInTime::Present => match self.accounts.get(address) {
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
                if let Some(((addr, _), account_info)) = self
                    .accounts_history
                    .iter_from((address.clone(), *block_number), rocksdb::Direction::Reverse)
                    .next()
                {
                    if address == &addr {
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
            BlockSelection::Number(number) => self.blocks_by_number.get(number),
            BlockSelection::Hash(hash) => {
                let block_number = self.blocks_by_hash.get(hash).unwrap_or_default();
                self.blocks_by_number.get(&block_number)
            }
        };
        match block {
            Some(block) => {
                tracing::trace!(?selection, ?block, "block found");
                Some(block)
            }
            None => None,
        }
    }

    /// Writes accounts to state (does not write to account history)
    pub fn write_accounts(&self, accounts: Vec<Account>, block_number: BlockNumber) {
        let mut account_batch = vec![];
        for account in accounts {
            account_batch.push((
                account.address,
                AccountInfo {
                    balance: account.balance,
                    nonce: account.nonce,
                    bytecode: account.bytecode,
                    code_hash: account.code_hash,
                },
            ));
        }

        self.accounts.insert_batch(account_batch, Some(block_number.into()));
    }

    /// Writes slots to state (does not write to slot history)
    pub fn write_slots(&self, slots: Vec<(Address, Slot)>, block_number: BlockNumber) {
        let mut slot_batch = vec![];

        for (address, slot) in slots {
            slot_batch.push(((address, slot.index), slot.value));
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
}

impl fmt::Debug for RocksStorageState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RocksDb").field("db", &"Arc<DB>").finish()
    }
}
