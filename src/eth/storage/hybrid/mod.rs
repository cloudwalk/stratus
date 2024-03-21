mod hybrid_state;
mod query_executor;

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;
use async_trait::async_trait;
use metrics::atomics::AtomicU64;
use num_traits::cast::ToPrimitive;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Json;
use sqlx::Pool;
use sqlx::Postgres;
use sqlx::QueryBuilder;
use sqlx::Row;
use tokio::sync::mpsc;
use tokio::sync::mpsc::channel;
use tokio::sync::Mutex;
use tokio::sync::MutexGuard;
use tokio::sync::RwLock;
use tokio::sync::RwLockReadGuard;
use tokio::sync::RwLockWriteGuard;
use tokio::task::JoinSet;

use self::hybrid_state::HybridStorageState;
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
use crate::eth::storage::hybrid::hybrid_state::AccountInfo;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::StorageError;

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
    tasks_pending: Arc<Mutex<()>>,
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
        let tasks_pending = Arc::new(Mutex::new(()));
        let worker_tasks_pending = Arc::clone(&tasks_pending);
        let (task_sender, task_receiver) = channel::<BlockTask>(32);
        let worker_pool = Arc::new(connection_pool.clone());
        let pool = Arc::clone(&worker_pool);
        tokio::spawn(async move {
            // Assuming you define a 'response_sender' if you plan to handle responses
            let worker_pool = Arc::<sqlx::Pool<sqlx::Postgres>>::clone(&worker_pool);
            // Omitting response channel setup for simplicity
            Self::worker(task_receiver, worker_pool, config.connections, worker_tasks_pending).await;
        });

        let block_number = Self::preload_block_number(connection_pool.clone()).await?;
        let state = RwLock::new(HybridStorageState::default());
        state.write().await.load_latest_data(&pool).await?;
        Ok(Self {
            state,
            block_number,
            task_sender,
            pool,
            tasks_pending,
        })
    }

    async fn preload_block_number(pool: Pool<sqlx::Postgres>) -> anyhow::Result<AtomicU64> {
        let blocks = sqlx::query!("SELECT block_number FROM neo_blocks ORDER BY block_number DESC LIMIT 1")
            .fetch_all(&pool)
            .await?;

        let last_block_number = blocks.last().map(|b| b.block_number.to_u64()).unwrap_or(Some(0)).unwrap_or(0);

        Ok(last_block_number.into())
    }

    async fn worker(
        mut receiver: tokio::sync::mpsc::Receiver<BlockTask>,
        pool: Arc<sqlx::Pool<sqlx::Postgres>>,
        connections: u32,
        tasks_pending: Arc<Mutex<()>>,
    ) {
        // Define the maximum number of concurrent tasks. Adjust this number based on your requirements.
        let max_concurrent_tasks: usize = (connections).try_into().unwrap_or(10usize);
        tracing::info!("Starting worker with max_concurrent_tasks: {}", max_concurrent_tasks);
        let mut futures = JoinSet::new();
        let mut pending_tasks_guard = None;
        while let Ok(block_task_opt) = recv_block_task(&mut receiver, &mut pending_tasks_guard, !futures.is_empty()).await {
            if let Some(block_task) = block_task_opt {
                let pool_clone = Arc::clone(&pool);

                if futures.len() < max_concurrent_tasks {
                    futures.spawn(query_executor::commit_eventually(pool_clone, block_task));

                    if pending_tasks_guard.is_none() {
                        pending_tasks_guard = Some(tasks_pending.lock().await);
                    }
                } else if let Some(_res) = futures.join_next().await {
                    futures.spawn(query_executor::commit_eventually(pool_clone, block_task));
                }
            } else {
                let timeout = Duration::from_millis(100);
                tokio::time::sleep(timeout).await;
                while futures.try_join_next().is_some() {}
            }
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
                    bytecode as "bytecode: _",
                    code_hash as "code_hash: _"
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
        let result: Option<_> = sqlx::query!(
            r#"
            SELECT transaction_data
            FROM neo_transactions
            WHERE hash = $1
        "#,
            hash as _
        )
        .fetch_optional(&*self.pool)
        .await
        .context("failed to select tx")?;

        match result {
            Some(record) => Ok(Some(serde_json::from_value(record.transaction_data)?)),
            _ => Ok(None),
        }
    }

    async fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        tracing::debug!(?filter, "reading logs");

        let log_query_builder = &mut QueryBuilder::new(
            r#"
                SELECT log_data
                FROM neo_logs
            "#,
        );
        log_query_builder.push(" WHERE block_number >= ").push_bind(filter.from_block);

        // verifies if to_block exists
        if let Some(block_number) = filter.to_block {
            log_query_builder.push(" AND block_number <= ").push_bind(block_number);
        }

        for address in filter.addresses.iter() {
            log_query_builder.push(" AND address = ").push_bind(address);
        }

        let log_query = log_query_builder.build();

        let query_result = log_query.fetch_all(&*self.pool).await?;

        let pg_logs = query_result
            .into_iter()
            .map(|row| {
                let json: Json<LogMined> = row.get("log_data");
                json.0
            })
            .filter(|log| filter.matches(log))
            .collect();

        Ok(pg_logs)
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
        state.blocks_by_number.truncate(600);
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
                    code_hash: account.code_hash.clone(),
                    slots: HashMap::new(),
                },
            );
            accounts_changes.0.push(BlockNumber::from(0));
            accounts_changes.1.push(account.address);
            accounts_changes.2.push(account.bytecode);
            accounts_changes.3.push(account.balance);
            accounts_changes.4.push(account.nonce);
        }

        let _ = self.tasks_pending.lock().await;
        sqlx::query!(
            "INSERT INTO public.neo_accounts (block_number, address, bytecode, balance, nonce)
            SELECT * FROM UNNEST($1::bigint[], $2::bytea[], $3::bytea[], $4::numeric[], $5::numeric[])
            AS t(block_number, address, bytecode, balance, nonce)
            ON CONFLICT (address,block_number) DO NOTHING;",
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

        let _ = self.tasks_pending.lock().await;

        sqlx::query!(r#"DELETE FROM neo_blocks WHERE block_number > $1"#, block_number as _)
            .execute(&*self.pool)
            .await?;
        sqlx::query!(r#"DELETE FROM neo_accounts WHERE block_number > $1"#, block_number as _)
            .execute(&*self.pool)
            .await?;
        sqlx::query!(r#"DELETE FROM neo_account_slots WHERE block_number > $1"#, block_number as _)
            .execute(&*self.pool)
            .await?;
        sqlx::query!(r#"DELETE FROM neo_transactions WHERE block_number > $1"#, block_number as _)
            .execute(&*self.pool)
            .await?;
        sqlx::query!(r#"DELETE FROM neo_logs WHERE block_number > $1"#, block_number as _)
            .execute(&*self.pool)
            .await?;

        state.accounts.clear();
        state.load_latest_data(&self.pool).await?;

        Ok(())
    }

    async fn read_slots_sample(&self, _start: BlockNumber, _end: BlockNumber, _max_samples: u64, _seed: u64) -> anyhow::Result<Vec<SlotSample>> {
        todo!()
    }
}

/// This function blocks if the mpsc is empty AND either:
/// 1. We have the pending_tasks_guard and there are no tasks pending
/// 2. We don't have the pending_tasks_guard
/// Otherwise this function is non-blocking until we can finish the pending tasks and release the lock.
async fn recv_block_task(
    receiver: &mut tokio::sync::mpsc::Receiver<BlockTask>,
    pending_tasks_guard: &mut Option<MutexGuard<'_, ()>>,
    pending_tasks: bool,
) -> anyhow::Result<Option<BlockTask>> {
    match receiver.try_recv() {
        Ok(block_task) => Ok(Some(block_task)),
        Err(mpsc::error::TryRecvError::Empty) =>
            if pending_tasks_guard.is_some() {
                if !pending_tasks {
                    let guard = std::mem::take(pending_tasks_guard);
                    drop(guard);
                    Ok(receiver.recv().await)
                } else {
                    Ok(None)
                }
            } else {
                Ok(receiver.recv().await)
            },
        Err(mpsc::error::TryRecvError::Disconnected) => Err(anyhow!(mpsc::error::TryRecvError::Disconnected)),
    }
}
