//! In-memory storage implementations.

use num_traits::cast::ToPrimitive;
use sqlx::Postgres;
use sqlx::QueryBuilder;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use indexmap::IndexMap;
use metrics::atomics::AtomicU64;
use rand::rngs::StdRng;
use rand::seq::IteratorRandom;
use rand::SeedableRng;
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use sqlx::Pool;
use tokio::sync::mpsc;
use tokio::sync::mpsc::channel;
use tokio::sync::RwLock;
use tokio::sync::RwLockReadGuard;
use tokio::sync::RwLockWriteGuard;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::ExecutionConflicts;
use crate::eth::primitives::ExecutionConflictsBuilder;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotSample;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::Wei;
use crate::eth::storage::inmemory::InMemoryHistory;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::StorageError;

#[derive(Debug)]
struct BlockTask {
    block_number: BlockNumber,
    block_hash: Hash,
    block_data: Block,
    account_changes: Vec<ExecutionAccountChanges>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct HybridPermanentStorageState {
    pub accounts: HashMap<Address, InMemoryPermanentAccount>,
    pub transactions: HashMap<Hash, TransactionMined>,
    pub blocks_by_number: IndexMap<BlockNumber, Arc<Block>>,
    pub blocks_by_hash: IndexMap<Hash, Arc<Block>>,
    pub logs: Vec<LogMined>,
}

#[derive(Debug)]
pub struct HybridPermanentStorage {
    state: RwLock<HybridPermanentStorageState>,
    block_number: AtomicU64,
    pool: sqlx::PgPool,
    task_sender: mpsc::Sender<BlockTask>,
}

#[derive(Debug)]
pub struct HybridPermanentStorageConfig {
    pub url: String,
    pub connections: u32,
    pub acquire_timeout: Duration,
}

impl HybridPermanentStorage {
    pub async fn new(config: HybridPermanentStorageConfig) -> anyhow::Result<Self> {
        tracing::info!("starting hybrid storage");

        let connection_pool = PgPoolOptions::new()
            .min_connections(config.connections)
            .max_connections(config.connections)
            .acquire_timeout(config.acquire_timeout)
            .connect(&config.url)
            .await
            .map_err(|e| {
                tracing::error!(reason = ?e, "failed to start postgres client");
                anyhow::anyhow!("failed to start postgres client")
            })?;

        let (task_sender, task_receiver) = channel::<BlockTask>(64);
        let pool = Arc::new(connection_pool.clone());
        tokio::spawn(async move {
            // Assuming you define a 'response_sender' if you plan to handle responses
            let worker_pool = Arc::<sqlx::Pool<sqlx::Postgres>>::clone(&pool);
            // Omitting response channel setup for simplicity
            Self::worker(task_receiver, worker_pool).await;
        });


        let block_number = Self::preload_block_number(connection_pool.clone()).await?;
        let state = RwLock::new(HybridPermanentStorageState::default());

        Ok(Self {
            state,
            block_number,
            task_sender,
            pool: connection_pool,
        })
    }

    async fn preload_block_number(pool: Pool<sqlx::Postgres>) -> anyhow::Result<AtomicU64> {
        let blocks = sqlx::query!("SELECT block_number FROM neo_blocks ORDER BY block_number DESC LIMIT 1")
            .fetch_all(&pool)
            .await?;

        let last_block_number = blocks.last().map(|b| b.block_number.to_u64()).unwrap_or(Some(0)).unwrap_or(0);

        Ok(last_block_number.into())
    }

    async fn worker(mut receiver: tokio::sync::mpsc::Receiver<BlockTask>, pool: Arc<sqlx::Pool<sqlx::Postgres>>) {
        tracing::info!("Starting worker");
        while let Some(block_task) = receiver.recv().await {
            let pool_clone = Arc::<sqlx::Pool<sqlx::Postgres>>::clone(&pool);

            let block_data = serde_json::to_value(block_task.block_data).unwrap();
            let account_changes = serde_json::to_value(block_task.account_changes.clone()).unwrap();

            // Here we attempt to insert the block data into the database.
            // Adjust the SQL query according to your table schema.
            // tokio::spawn(async move {
                 let result = sqlx::query!(
                     "INSERT INTO neo_blocks (block_number, block_hash, block, account_changes, created_at) VALUES ($1, $2, $3, $4, NOW());",
                     block_task.block_number as _,
                     block_task.block_hash as _,
                     block_data,
                     account_changes
                 )
                 .execute(&*pool_clone)
                 .await;

                 // Handle the result of the insert operation.
                 match result {
                     Ok(_) => tracing::info!("Block {} inserted successfully.", block_task.block_number),
                     Err(e) => tracing::error!("Failed to insert block {}: {}", block_task.block_number, e),
                 }

                let mut accounts_changes: (Vec<i64>, Vec<Address>, Vec<Option<Bytes>>, Vec<Wei>, Vec<Nonce>) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());

                for changes in block_task.account_changes {
                    let (original_nonce, new_nonce) = changes.nonce.take_both();
                    let (original_balance, new_balance) = changes.balance.take_both();

                    let original_nonce = original_nonce.unwrap_or_default();
                    let original_balance = original_balance.unwrap_or_default();

                    let nonce = new_nonce.clone().unwrap_or(original_nonce.clone());
                    let balance = new_balance.clone().unwrap_or(original_balance.clone());

                    let bytecode = changes.bytecode.take().unwrap_or_else(|| {
                        tracing::debug!("bytecode not set, defaulting to None");
                        None
                    });

                    accounts_changes.0.push(block_task.block_number.clone().as_i64());
                    accounts_changes.1.push(changes.address.clone());
                    accounts_changes.2.push(bytecode);
                    accounts_changes.3.push(balance);
                    accounts_changes.4.push(nonce);
                }


                if accounts_changes.0.len() > 0 {
                    dbg!(&accounts_changes);
                    let result = sqlx::query!(
                        "INSERT INTO public.neo_accounts (block_number, address, bytecode, balance, nonce)
                        SELECT * FROM UNNEST($1::bigint[], $2::bytea[], $3::bytea[], $4::numeric[], $5::numeric[])
                        AS t(block_number, address, bytecode, balance, nonce);",
                        accounts_changes.0 as _,
                        accounts_changes.1 as _,
                        accounts_changes.2 as _,
                        accounts_changes.3 as _,
                        accounts_changes.4 as _,
                    ).execute(&*pool_clone).await;

                    match result {
                        Ok(_) => println!("Accounts inserted successfully."),
                        Err(e) => println!("Failed to insert accounts: {}", e),
                    }
                }
            // });
        }
    }

    // -------------------------------------------------------------------------
    // Lock methods
    // -------------------------------------------------------------------------

    /// Locks inner state for reading.
    async fn lock_read(&self) -> RwLockReadGuard<'_, HybridPermanentStorageState> {
        self.state.read().await
    }

    /// Locks inner state for writing.
    async fn lock_write(&self) -> RwLockWriteGuard<'_, HybridPermanentStorageState> {
        self.state.write().await
    }

    // -------------------------------------------------------------------------
    // State methods
    // -------------------------------------------------------------------------

    /// Clears in-memory state.
    pub async fn clear(&self) {
        let mut state = self.lock_write().await;
        state.accounts.clear();
        state.transactions.clear();
        state.blocks_by_hash.clear();
        state.blocks_by_number.clear();
        state.logs.clear();
    }

    async fn check_conflicts(state: &HybridPermanentStorageState, account_changes: &[ExecutionAccountChanges]) -> Option<ExecutionConflicts> {
        let mut conflicts = ExecutionConflictsBuilder::default();

        for change in account_changes {
            let address = &change.address;

            if let Some(account) = state.accounts.get(address) {
                // check account info conflicts
                if let Some(original_nonce) = change.nonce.take_original_ref() {
                    let account_nonce = account.nonce.get_current_ref();
                    if original_nonce != account_nonce {
                        conflicts.add_nonce(address.clone(), account_nonce.clone(), original_nonce.clone());
                    }
                }
                if let Some(original_balance) = change.balance.take_original_ref() {
                    let account_balance = account.balance.get_current_ref();
                    if original_balance != account_balance {
                        conflicts.add_balance(address.clone(), account_balance.clone(), original_balance.clone());
                    }
                }

                // check slots conflicts
                for (slot_index, slot_change) in &change.slots {
                    if let Some(account_slot) = account.slots.get(slot_index).map(|value| value.get_current_ref()) {
                        if let Some(original_slot) = slot_change.take_original_ref() {
                            let account_slot_value = account_slot.value.clone();
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

impl HybridPermanentStorage {}

#[async_trait]
impl PermanentStorage for HybridPermanentStorage {
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
        tracing::debug!(%address, "reading account");

        let state = self.lock_read().await;

        match state.accounts.get(address) {
            Some(inmemory_account) => {
                let account = inmemory_account.to_account(point_in_time);
                tracing::trace!(%address, ?account, "account found");
                Ok(Some(account))
            }

            None => {
                tracing::trace!(%address, "account not found");
                Ok(None)
            }
        }
    }

    async fn maybe_read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        tracing::debug!(%address, %slot_index, ?point_in_time, "reading slot");

        let state = self.lock_read().await;
        let Some(account) = state.accounts.get(address) else {
            tracing::trace!(%address, "account not found");
            return Ok(Default::default());
        };

        match account.slots.get(slot_index) {
            Some(slot_history) => {
                let slot = slot_history.get_at_point(point_in_time).unwrap_or_default();
                tracing::trace!(%address, %slot_index, ?point_in_time, %slot, "slot found");
                Ok(Some(slot))
            }

            None => {
                tracing::trace!(%address, %slot_index, ?point_in_time, "slot not found");
                Ok(None)
            }
        }
    }

    async fn read_block(&self, selection: &BlockSelection) -> anyhow::Result<Option<Block>> {
        tracing::debug!(?selection, "reading block");

        let state_lock = self.lock_read().await;
        let block = match selection {
            BlockSelection::Latest => state_lock.blocks_by_number.values().last().cloned(),
            BlockSelection::Earliest => state_lock.blocks_by_number.values().next().cloned(),
            BlockSelection::Number(number) => state_lock.blocks_by_number.get(number).cloned(),
            BlockSelection::Hash(hash) => state_lock.blocks_by_hash.get(hash).cloned(),
        };
        match block {
            Some(block) => {
                tracing::trace!(?selection, ?block, "block found");
                Ok(Some((*block).clone()))
            }
            None => {
                tracing::trace!(?selection, "block not found");
                Ok(None)
            }
        }
    }

    async fn read_mined_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        tracing::debug!(%hash, "reading transaction");
        let state_lock = self.lock_read().await;

        match state_lock.transactions.get(hash) {
            Some(transaction) => {
                tracing::trace!(%hash, "transaction found");
                Ok(Some(transaction.clone()))
            }
            None => {
                tracing::trace!(%hash, "transaction not found");
                Ok(None)
            }
        }
    }

    async fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        tracing::debug!(?filter, "reading logs");
        let state_lock = self.lock_read().await;

        let logs = state_lock
            .logs
            .iter()
            .skip_while(|log| log.block_number < filter.from_block)
            .take_while(|log| match filter.to_block {
                Some(to_block) => log.block_number <= to_block,
                None => true,
            })
            .filter(|log| filter.matches(log))
            .cloned()
            .collect();
        Ok(logs)
    }

    async fn save_block(&self, block: Block) -> anyhow::Result<(), StorageError> {
        let mut state = self.lock_write().await;

        // check conflicts before persisting any state changes
        let account_changes = block.compact_account_changes();
        if let Some(conflicts) = Self::check_conflicts(&state, &account_changes).await {
            return Err(StorageError::Conflict(conflicts));
        }

        // save block
        tracing::debug!(number = %block.number(), "saving block");
        let block = Arc::new(block);
        let number = block.number();
        state.blocks_by_number.insert(*number, Arc::clone(&block));
        state.blocks_by_hash.insert(block.hash().clone(), Arc::clone(&block));

        // save transactions
        for transaction in block.transactions.clone() {
            tracing::debug!(hash = %transaction.input.hash, "saving transaction");
            state.transactions.insert(transaction.input.hash.clone(), transaction.clone());
            if transaction.is_success() {
                for log in transaction.logs {
                    state.logs.push(log);
                }
            }
        }

        // save block account changes
        for changes in account_changes.clone() {
            let account = state
                .accounts
                .entry(changes.address.clone())
                .or_insert_with(|| InMemoryPermanentAccount::new_empty(changes.address));

            // account basic info
            if let Some(nonce) = changes.nonce.take_modified() {
                account.nonce.push(*number, nonce);
            }
            if let Some(balance) = changes.balance.take_modified() {
                account.balance.push(*number, balance);
            }

            // bytecode
            if let Some(Some(bytecode)) = changes.bytecode.take_modified() {
                account.bytecode.push(*number, Some(bytecode));
            }

            // slots
            // XXX change here to a key value where the key is the slot index+address
            for (_, slot) in changes.slots {
                if let Some(slot) = slot.take_modified() {
                    match account.slots.get_mut(&slot.index) {
                        Some(slot_history) => {
                            slot_history.push(*number, slot);
                        }
                        None => {
                            account.slots.insert(slot.index.clone(), InMemoryHistory::new(*number, slot));
                        }
                    }
                }
            }
        }

        let block_task = BlockTask {
            block_number: block.number().clone(),
            block_hash: block.hash().clone(),
            block_data: (*block).clone(),
            account_changes, //TODO make account changes work from postgres then we can load it on memory
        };

        self.task_sender.send(block_task).await.expect("Failed to send block task");

        Ok(())
    }

    async fn after_commit_hook(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        tracing::debug!(?accounts, "saving initial accounts");

        let mut state = self.lock_write().await;
        for account in accounts {
            state.accounts.insert(
                account.address.clone(),
                InMemoryPermanentAccount::new_with_balance(account.address, account.balance),
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

        // remove blocks
        let mut state = self.lock_write().await;
        state.blocks_by_hash.retain(|_, b| *b.number() <= block_number);
        state.blocks_by_number.retain(|_, b| *b.number() <= block_number);

        // remove transactions and logs
        state.transactions.retain(|_, t| t.block_number <= block_number);
        state.logs.retain(|l| l.block_number <= block_number);

        // remove account changes
        for account in state.accounts.values_mut() {
            account.reset_at(block_number);
        }

        Ok(())
    }

    async fn read_slots_sample(&self, start: BlockNumber, end: BlockNumber, max_samples: u64, seed: u64) -> anyhow::Result<Vec<SlotSample>> {
        let state = self.lock_read().await;

        let samples = state
            .accounts
            .iter()
            .filter(|(_, account_info)| account_info.is_contract())
            .flat_map(|(_, contract)| {
                contract
                    .slots
                    .values()
                    .flat_map(|slot_history| Vec::from((*slot_history).clone()))
                    .filter_map(|slot| {
                        if slot.block_number >= start && slot.block_number < end {
                            Some(SlotSample {
                                address: contract.address.clone(),
                                block_number: slot.block_number,
                                index: slot.value.index,
                                value: slot.value.value,
                            })
                        } else {
                            None
                        }
                    })
            });

        match max_samples {
            0 => Ok(samples.collect()),
            n => {
                let mut rng = StdRng::seed_from_u64(seed);
                Ok(samples.choose_multiple(&mut rng, n as usize))
            }
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct InMemoryPermanentAccount {
    #[allow(dead_code)]
    pub address: Address,
    pub balance: InMemoryHistory<Wei>,
    pub nonce: InMemoryHistory<Nonce>,
    pub bytecode: InMemoryHistory<Option<Bytes>>,
    pub slots: HashMap<SlotIndex, InMemoryHistory<Slot>>,
}

impl InMemoryPermanentAccount {
    /// Creates a new empty permanent account.
    fn new_empty(address: Address) -> Self {
        Self::new_with_balance(address, Wei::ZERO)
    }

    /// Creates a new permanent account with initial balance.
    pub fn new_with_balance(address: Address, balance: Wei) -> Self {
        Self {
            address,
            balance: InMemoryHistory::new_at_zero(balance),
            nonce: InMemoryHistory::new_at_zero(Nonce::ZERO),
            bytecode: InMemoryHistory::new_at_zero(None),
            slots: Default::default(),
        }
    }

    /// Resets all account changes to the specified block number.
    pub fn reset_at(&mut self, block_number: BlockNumber) {
        // SAFETY: ok to unwrap because all historical values starts at block 0
        self.balance = self.balance.reset_at(block_number).expect("never empty");
        self.nonce = self.nonce.reset_at(block_number).expect("never empty");
        self.bytecode = self.bytecode.reset_at(block_number).expect("never empty");

        // SAFETY: not ok to unwrap because slot value does not start at block 0
        let mut new_slots = HashMap::with_capacity(self.slots.len());
        for (slot_index, slot_history) in self.slots.iter() {
            if let Some(new_slot_history) = slot_history.reset_at(block_number) {
                new_slots.insert(slot_index.clone(), new_slot_history);
            }
        }
        self.slots = new_slots;
    }

    /// Checks current account is a contract.
    pub fn is_contract(&self) -> bool {
        self.bytecode.get_current().is_some()
    }
    /// Converts itself to an account at a point-in-time.
    pub fn to_account(&self, point_in_time: &StoragePointInTime) -> Account {
        Account {
            address: self.address.clone(),
            balance: self.balance.get_at_point(point_in_time).unwrap_or_default(),
            nonce: self.nonce.get_at_point(point_in_time).unwrap_or_default(),
            bytecode: self.bytecode.get_at_point(point_in_time).unwrap_or_default(),
        }
    }
}
