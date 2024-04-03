mod hybrid_state;
mod query_executor;
mod rocks_db;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use futures::future::join_all;
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
use tokio::sync::Semaphore;
use tokio::sync::SemaphorePermit;
use tokio::task::JoinSet;
use tokio::time::sleep;

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
    state: HybridStorageState, //XXX TODO remove RwLock when rocksdb is implemented everywhere
    pool: Arc<Pool<Postgres>>,
    block_number: AtomicU64,
    task_sender: mpsc::Sender<BlockTask>,
    tasks_pending: Arc<Semaphore>, // TODO change to Mutex<()>
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
        let tasks_pending = Arc::new(Semaphore::new(1));
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
        let state = HybridStorageState::new();
        state.load_latest_data(&pool).await?;
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
        tasks_pending: Arc<Semaphore>,
    ) {
        // Define the maximum number of concurrent tasks. Adjust this number based on your requirements.
        let max_concurrent_tasks: usize = (connections).try_into().unwrap_or(10usize);
        tracing::info!("Starting worker with max_concurrent_tasks: {}", max_concurrent_tasks);
        let mut futures = JoinSet::new();
        let mut permit = None;
        while let Some(block_task) = recv_block_task(&mut receiver, &mut permit).await {
            let pool_clone = Arc::clone(&pool);
            if futures.len() < max_concurrent_tasks {
                futures.spawn(query_executor::commit_eventually(pool_clone, block_task));
                if permit.is_none() {
                    permit = Some(tasks_pending.acquire().await.expect("semaphore has closed"));
                }
            } else if let Some(_res) = futures.join_next().await {
                futures.spawn(query_executor::commit_eventually(pool_clone, block_task));
            }
        }
    }

    // -------------------------------------------------------------------------
    // State methods
    // -------------------------------------------------------------------------

    /// Clears in-memory state.
    pub async fn clear(&self) {
        let _ = self.state.accounts.clear();
        let _ = self.state.accounts_history.clear();
        let _ = self.state.account_slots.clear();
        let _ = self.state.account_slots_history.clear();

        self.state.transactions.clear().unwrap();
        self.state.blocks_by_hash.clear().unwrap();
        self.state.blocks_by_number.clear().unwrap();
        self.state.logs.clear().unwrap();
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
        let account = match point_in_time {
            StoragePointInTime::Present => {
                match self.state.accounts.get(address) {
                    Some(inmemory_account) => {
                        let account = inmemory_account.to_account(address).await;
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
                {
                    sqlx::query_as!(
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
                    .context("failed to select account")?
                }
            }
        };
        Ok(account)
    }

    async fn maybe_read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        tracing::debug!(%address, %slot_index, ?point_in_time, "reading slot");
        self.state.get_slot_at_point(address, slot_index, point_in_time, &self.pool).await
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
            None => {
                let block_query = &mut QueryBuilder::new(
                    r#"
                    SELECT block
                    FROM neo_blocks
                "#,
                );
                match selection {
                    BlockSelection::Latest => block_query.push(" WHERE block_number = (SELECT MAX(block_number) FROM neo_blocks)"),
                    BlockSelection::Earliest => block_query.push(" WHERE block_number = 0"),
                    BlockSelection::Number(number) => block_query.push(" WHERE block_number = ").push_bind(number),
                    BlockSelection::Hash(hash) => block_query.push(" WHERE block_hash = ").push_bind(hash),
                };
                Ok(block_query
                    .build()
                    .fetch_optional(&*self.pool)
                    .await?
                    .map(|row| row.get::<Json<Block>, _>("block").0))
            }
        }
    }

    async fn read_mined_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        tracing::debug!(%hash, "reading transaction");

        match self.state.transactions.get(hash) {
            Some(transaction) => {
                tracing::trace!(%hash, "transaction found in memory");
                Ok(Some(transaction.clone()))
            }
            None => {
                let result = sqlx::query!(
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
                    Some(record) => {
                        tracing::trace!(%hash, "transaction found in postgres");
                        Ok(Some(serde_json::from_value(record.transaction_data)?))
                    }
                    _ => {
                        tracing::trace!(%hash, "transaction not found");
                        Ok(None)
                    }
                }
            }
        }
    }

    async fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        tracing::debug!(?filter, "reading logs");
        self.state.read_logs(filter, &self.pool).await
    }

    async fn save_block(&self, block: Block) -> anyhow::Result<(), StorageError> {
        // check conflicts before persisting any state changes
        let account_changes = block.compact_account_changes();
        if let Some(conflicts) = Self::check_conflicts(&self.state, &account_changes).await {
            return Err(StorageError::Conflict(conflicts));
        }

        let mut futures = Vec::with_capacity(9);

        let mut txs_batch = vec![];
        let mut logs_batch = vec![];
        for transaction in block.transactions.clone() {
            txs_batch.push((transaction.input.hash.clone(), transaction.clone()));
            for log in transaction.logs {
                logs_batch.push(((transaction.input.hash.clone(), log.log_index), log.clone()));
            }
        }

        let txs_rocks = Arc::clone(&self.state.transactions);
        let logs_rocks = Arc::clone(&self.state.logs);
        futures.push(tokio::task::spawn_blocking(move || txs_rocks.insert_batch(txs_batch)));
        futures.push(tokio::task::spawn_blocking(move || logs_rocks.insert_batch(logs_batch)));

        // save block
        let number = *block.number();
        let hash = block.hash().clone();

        let blocks_by_number = Arc::clone(&self.state.blocks_by_number);
        let blocks_by_hash = Arc::clone(&self.state.blocks_by_hash);
        let block_clone = block.clone();
        let hash_clone = hash.clone();
        futures.push(tokio::task::spawn_blocking(move || blocks_by_number.insert(number, block_clone)));
        futures.push(tokio::task::spawn_blocking(move || blocks_by_hash.insert(hash_clone, number)));

        futures.append(
            &mut self
                .state
                .update_state_with_execution_changes(&account_changes, number)
                .await
                .context("failed to update state with execution changes")?,
        );

        let block_task = BlockTask {
            block_number: number,
            block_hash: hash,
            block_data: block,
            account_changes, //TODO make account changes work from postgres then we can load it on memory
        };

        self.task_sender.send(block_task).await.expect("Failed to send block task");
        join_all(futures).await;
        Ok(())
    }

    async fn after_commit_hook(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        sleep(Duration::from_secs(2)).await;
        tracing::debug!(?accounts, "saving initial accounts");

        let mut accounts_changes = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
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
            accounts_changes.0.push(BlockNumber::from(0));
            accounts_changes.1.push(account.address);
            accounts_changes.2.push(account.bytecode);
            accounts_changes.3.push(account.balance);
            accounts_changes.4.push(account.nonce);
            accounts_changes.5.push(account.code_hash);
        }

        let _ = self.tasks_pending.acquire().await.expect("semaphore has closed");
        sqlx::query!(
            "INSERT INTO public.neo_accounts (block_number, address, bytecode, balance, nonce, code_hash)
            SELECT * FROM UNNEST($1::bigint[], $2::bytea[], $3::bytea[], $4::numeric[], $5::numeric[], $6::bytea[])
            AS t(block_number, address, bytecode, balance, nonce, code_hash)
            ON CONFLICT (address,block_number) DO NOTHING;",
            accounts_changes.0 as _,
            accounts_changes.1 as _,
            accounts_changes.2 as _,
            accounts_changes.3 as _,
            accounts_changes.4 as _,
            accounts_changes.5 as _,
        )
        .execute(&*self.pool)
        .await?;
        Ok(())
    }

    async fn reset_at(&self, block_number: BlockNumber) -> anyhow::Result<()> {
        sleep(Duration::from_secs(2)).await;
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
        self.state.blocks_by_hash.clear()?;
        self.state.blocks_by_number.clear()?;

        // remove transactions and logs
        self.state.transactions.clear()?;
        self.state.logs.clear()?;

        let _ = self.tasks_pending.acquire().await.expect("semaphore has closed");

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

        let _ = self.state.accounts.clear();
        let _ = self.state.accounts_history.clear();
        let _ = self.state.account_slots.clear();
        let _ = self.state.account_slots_history.clear();

        self.state.load_latest_data(&self.pool).await?;

        Ok(())
    }

    async fn read_slots_sample(&self, _start: BlockNumber, _end: BlockNumber, _max_samples: u64, _seed: u64) -> anyhow::Result<Vec<SlotSample>> {
        todo!()
    }
}

async fn recv_block_task(receiver: &mut tokio::sync::mpsc::Receiver<BlockTask>, permit: &mut Option<SemaphorePermit<'_>>) -> Option<BlockTask> {
    match receiver.try_recv() {
        Ok(block_task) => Some(block_task),
        Err(mpsc::error::TryRecvError::Empty) => {
            if permit.is_some() {
                let perm = std::mem::take(permit);
                drop(perm);
            }
            receiver.recv().await
        }
        Err(_) => None,
    }
}
