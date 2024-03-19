mod hybrid_history;
mod query_executor;

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use metrics::atomics::AtomicU64;
use num_traits::cast::ToPrimitive;
use sqlx::postgres::PgPoolOptions;
use sqlx::Pool;
use sqlx::Postgres;
use tokio::sync::mpsc;
use tokio::sync::mpsc::channel;
use tokio::sync::RwLock;
use tokio::sync::RwLockReadGuard;
use tokio::sync::RwLockWriteGuard;
use tokio::sync::Semaphore;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::ExecutionConflicts;
use crate::eth::primitives::ExecutionConflictsBuilder;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotSample;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::hybrid::hybrid_history::AccountInfo;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::StorageError;

use self::hybrid_history::HybridStorageState;

#[derive(Debug)]
struct BlockTask {
    block_number: BlockNumber,
    block_hash: Hash,
    block_data: Block,
    account_changes: Vec<ExecutionAccountChanges>,
}


#[derive(Debug)]
pub struct HybridPermanentStorage {
    state: RwLock<HybridStorageState>,
    pool: Arc<Pool<Postgres>>,
    block_number: AtomicU64,
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
        tracing::info!(?config, "starting hybrid storage");

        let connection_pool = PgPoolOptions::new()
            .min_connections(config.connections / 2)
            .max_connections(config.connections)
            .acquire_timeout(config.acquire_timeout)
            .connect(&config.url)
            .await
            .map_err(|e| {
                tracing::error!(reason = ?e, "failed to start postgres client");
                anyhow::anyhow!("failed to start postgres client")
            })?;

        let (task_sender, task_receiver) = channel::<BlockTask>(32);
        let worker_pool = Arc::new(connection_pool.clone());
        let pool = Arc::clone(&worker_pool);
        tokio::spawn(async move {
            // Assuming you define a 'response_sender' if you plan to handle responses
            let worker_pool = Arc::<sqlx::Pool<sqlx::Postgres>>::clone(&worker_pool);
            // Omitting response channel setup for simplicity
            Self::worker(task_receiver, worker_pool, config.connections).await;
        });

        let block_number = Self::preload_block_number(connection_pool.clone()).await?;
        let state = RwLock::new(HybridStorageState::default());
        state.write().await.load_latest_data(&pool).await?;
        Ok(Self {
            state,
            block_number,
            task_sender,
            pool
        })
    }

    async fn preload_block_number(pool: Pool<sqlx::Postgres>) -> anyhow::Result<AtomicU64> {
        let blocks = sqlx::query!("SELECT block_number FROM neo_blocks ORDER BY block_number DESC LIMIT 1")
            .fetch_all(&pool)
            .await?;

        let last_block_number = blocks.last().map(|b| b.block_number.to_u64()).unwrap_or(Some(0)).unwrap_or(0);

        Ok(last_block_number.into())
    }

    async fn worker(mut receiver: tokio::sync::mpsc::Receiver<BlockTask>, pool: Arc<sqlx::Pool<sqlx::Postgres>>, connections: u32) {
        // Define the maximum number of concurrent tasks. Adjust this number based on your requirements.
        let max_concurrent_tasks: usize = (connections).try_into().unwrap_or(10usize);
        tracing::info!("Starting worker with max_concurrent_tasks: {}", max_concurrent_tasks);
        let semaphore = Arc::new(Semaphore::new(max_concurrent_tasks));

        while let Some(block_task) = receiver.recv().await {
            let pool_clone = Arc::clone(&pool);
            let semaphore_clone = Arc::clone(&semaphore);

            tokio::spawn(async move {
                let permit = semaphore_clone.acquire_owned().await.expect("Failed to acquire semaphore permit");
                query_executor::commit_eventually(pool_clone, block_task).await;
                drop(permit);
            });
        }
    }

    // -------------------------------------------------------------------------
    // Lock methods
    // -------------------------------------------------------------------------

    /// Locks inner state for reading.
    async fn lock_read(&self) -> RwLockReadGuard<'_, HybridStorageState> {
        self.state.read().await
    }

    /// Locks inner state for writing.
    async fn lock_write(&self) -> RwLockWriteGuard<'_, HybridStorageState> {
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

    async fn check_conflicts(state: &HybridStorageState, account_changes: &[ExecutionAccountChanges]) -> Option<ExecutionConflicts> {
        let mut conflicts = ExecutionConflictsBuilder::default();

        for change in account_changes {
            let address = &change.address;

            if let Some(account) = hybrid_state.hybrid_accounts_slots.get(address) {
                // check account info conflicts
                if let Some(original_nonce) = change.nonce.take_original_ref() {
                    let account_nonce = &account.nonce;
                    let account_nonce = &account.nonce;
                    if original_nonce != account_nonce {
                        conflicts.add_nonce(address.clone(), account_nonce.clone(), original_nonce.clone());
                    }
                }
                if let Some(original_balance) = change.balance.take_original_ref() {
                    let account_balance = &account.balance;
                    let account_balance = &account.balance;
                    if original_balance != account_balance {
                        conflicts.add_balance(address.clone(), account_balance.clone(), original_balance.clone());
                    }
                }
                // check slots conflicts
                for (slot_index, slot_change) in &change.slots {
                    if let Some(value) = account.slots.get(slot_index) {
                    if let Some(value) = account.slots.get(slot_index) {
                        if let Some(original_slot) = slot_change.take_original_ref() {
                            let account_slot_value = value.value.clone();
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
        //XXX TODO deal with point_in_time first, e.g create to_account at hybrid_accounts_slots
        let state = self.state.read().await;

        let account = match point_in_time {
            StoragePointInTime::Present => {
                match state.accounts.get(address) {
                    Some(inmemory_account) => {
                        let account = inmemory_account.to_account(point_in_time, address).await;
                        tracing::trace!(%address, ?account, "account found");
                        Some(account)
                    }

                    None => {
                        //XXX TODO start a inmemory account from the database maybe using to_account on a empty account
                        tracing::trace!(%address, "account not found");
                        None
                    }
                }
            }
            StoragePointInTime::Past(block_number) => sqlx::query_as!(
                Account,
                r#"
                SELECT address as "address: _",
                    nonce as "nonce: _",
                    balance as "balance: _",
                    bytecode as "bytecode: _"
                FROM neo_accounts
                WHERE address = $1
                AND block_number = (SELECT MAX(block_number)
                                        FROM historical_slots
                                        WHERE address = $1
                                        AND block_number <= $2)"#,
                address as _,
                block_number as _
            )
            .fetch_optional(&*self.pool)
            .await
            .context("failed to select account")?,
        };
        Ok(account)
    }

    async fn maybe_read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        tracing::debug!(%address, %slot_index, ?point_in_time, "reading slot");
        self.state.read().await.get_slot_at_point(address, slot_index, point_in_time, &self.pool).await
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
        let mut state = self.state.write().await;

        // check conflicts before persisting any state changes
        let account_changes = block.compact_account_changes();
        if let Some(conflicts) = Self::check_conflicts(&state, &account_changes).await {
            return Err(StorageError::Conflict(conflicts));
        }

        // save block
        let block = Arc::new(block);
        let number = block.number();
        state.blocks_by_hash.truncate(600);
        state.blocks_by_number.insert(*number, Arc::clone(&block));

        //XXX deal with errors later
        let _ = state.update_state_with_execution_changes(&account_changes).await;

        let block_task = BlockTask {
            block_number: *block.number(),
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

        let mut state = self.state.write().await;
        let mut accounts_changes = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for account in accounts {
            state.accounts.insert(
                account.address.clone(),
                AccountInfo {
                    balance: account.balance.clone(),
                    nonce: account.nonce.clone(),
                    bytecode: account.bytecode.clone(),
                    slots: HashMap::new(),
                },
            );
            accounts_changes.0.push(BlockNumber::from(0));
            accounts_changes.1.push(account.address);
            accounts_changes.2.push(account.bytecode);
            accounts_changes.3.push(account.balance);
            accounts_changes.4.push(account.nonce);
        }
        tokio::time::sleep(Duration::from_secs(5)).await; // XXX Waiting for genesis to be commited pg
        sqlx::query!(
            "INSERT INTO public.neo_accounts (block_number, address, bytecode, balance, nonce)
            SELECT * FROM UNNEST($1::bigint[], $2::bytea[], $3::bytea[], $4::numeric[], $5::numeric[])
            AS t(block_number, address, bytecode, balance, nonce);",
            accounts_changes.0 as _,
            accounts_changes.1 as _,
            accounts_changes.2 as _,
            accounts_changes.3 as _,
            accounts_changes.4 as _,
        )
        .execute(&*self.pool)
        .await?;
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

        // XXX Reset in postgres and load latest account data
        state.accounts.clear();

        Ok(())
    }

    async fn read_slots_sample(&self, _start: BlockNumber, _end: BlockNumber, _max_samples: u64, _seed: u64) -> anyhow::Result<Vec<SlotSample>> {
        todo!()
    }
}
