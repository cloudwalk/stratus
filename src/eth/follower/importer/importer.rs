use std::borrow::Cow;
use std::cmp::min;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use alloy_rpc_types_eth::BlockTransactions;
use anyhow::anyhow;
use anyhow::bail;
use futures::StreamExt;
use futures::try_join;
use itertools::Itertools;
use tokio::sync::mpsc;
use tokio::task::yield_now;
use tokio::time::timeout;
use tracing::Span;

use crate::GlobalState;
use crate::eth::executor::Executor;
use crate::eth::follower::consensus::Consensus;
use crate::eth::miner::Miner;
use crate::eth::miner::miner::CommitItem;
use crate::eth::miner::miner::interval_miner::mine_and_commit;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalReceipts;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionError;
use crate::eth::storage::StratusStorage;
use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
use crate::ext::DisplayExt;
use crate::ext::SleepReason;
use crate::ext::spawn;
use crate::ext::traced_sleep;
use crate::globals::IMPORTER_ONLINE_TASKS_SEMAPHORE;
use crate::infra::BlockchainClient;
use crate::infra::kafka::KafkaConnector;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::tracing::SpanExt;
use crate::infra::tracing::warn_task_rx_closed;
use crate::infra::tracing::warn_task_tx_closed;
use crate::ledger::events::transaction_to_events;
use crate::log_and_err;
use crate::utils::DropTimer;
#[cfg(feature = "metrics")]
use crate::utils::calculate_tps;

#[derive(Clone, Copy)]
pub enum ImporterMode {
    /// A normal follower imports a mined block.
    NormalFollower,
    /// Fake leader feches a block, re-executes its txs and then mines it's own block.
    FakeLeader,
    /// Fetch a block with pre-computed changes
    BlockWithChanges,
}

// -----------------------------------------------------------------------------
// Globals
// -----------------------------------------------------------------------------

/// Current block number of the external RPC blockchain.
static EXTERNAL_RPC_CURRENT_BLOCK: AtomicU64 = AtomicU64::new(0);

/// Timestamp of when EXTERNAL_RPC_CURRENT_BLOCK was updated last.
static LATEST_FETCHED_BLOCK_TIME: AtomicU64 = AtomicU64::new(0);

/// Only sets the external RPC current block number if it is equals or greater than the current one.
fn set_external_rpc_current_block(new_number: BlockNumber) {
    LATEST_FETCHED_BLOCK_TIME.store(chrono::Utc::now().timestamp() as u64, Ordering::Relaxed);
    let _ = EXTERNAL_RPC_CURRENT_BLOCK.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current_number| {
        Some(current_number.max(new_number.as_u64()))
    });
}

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------
/// Number of blocks that are downloaded in parallel.
const PARALLEL_BLOCKS: usize = 3;

/// Timeout awaiting for newHeads event before fallback to polling.
const TIMEOUT_NEW_HEADS: Duration = Duration::from_millis(2000);

pub struct Importer {
    executor: Arc<Executor>,

    miner: Arc<Miner>,

    storage: Arc<StratusStorage>,

    chain: Arc<BlockchainClient>,

    sync_interval: Duration,

    kafka_connector: Option<Arc<KafkaConnector>>,

    importer_mode: ImporterMode,
}

impl Importer {
    pub fn new(
        executor: Arc<Executor>,
        miner: Arc<Miner>,
        storage: Arc<StratusStorage>,
        chain: Arc<BlockchainClient>,
        kafka_connector: Option<Arc<KafkaConnector>>,
        sync_interval: Duration,
        importer_mode: ImporterMode,
    ) -> Self {
        tracing::info!("creating importer");

        Self {
            executor,
            miner,
            storage,
            chain,
            sync_interval,
            kafka_connector,
            importer_mode,
        }
    }

    // -----------------------------------------------------------------------------
    // Shutdown
    // -----------------------------------------------------------------------------

    /// Checks if the importer should shutdown
    fn should_shutdown(task_name: &str) -> bool {
        GlobalState::is_shutdown_warn(task_name) || GlobalState::is_importer_shutdown_warn(task_name)
    }

    // -----------------------------------------------------------------------------
    // Execution
    // -----------------------------------------------------------------------------

    pub async fn run_importer_online(self: Arc<Self>) -> anyhow::Result<()> {
        let _timer = DropTimer::start("importer-online::run_importer_online");

        let storage = &self.storage;
        let number: BlockNumber = storage.read_block_number_to_resume_import()?;

        // Spawn common tasks: number fetcher
        let task_number_fetcher = spawn(
            "importer::number-fetcher",
            Importer::start_number_fetcher(Arc::clone(&self.chain), self.sync_interval),
        );

        let (exec_or_save_handle, block_fetcher_handle) = match self.importer_mode {
            ImporterMode::NormalFollower | ImporterMode::FakeLeader => {
                let (backlog_tx, backlog_rx) = mpsc::channel(10_000);

                let task_executor = spawn(
                    "importer::executor",
                    Importer::start_block_executor(
                        Arc::clone(&self.executor),
                        Arc::clone(&self.miner),
                        backlog_rx,
                        self.kafka_connector.clone(),
                        self.importer_mode,
                    ),
                );

                let task_block_fetcher = spawn(
                    "importer::block-fetcher",
                    Importer::start_block_fetcher(Arc::clone(&self.chain), backlog_tx, number),
                );

                (task_executor, task_block_fetcher)
            }
            ImporterMode::BlockWithChanges => {
                let (backlog_tx, backlog_rx) = mpsc::channel(10_000);

                let task_saver = spawn(
                    "importer::executor",
                    Importer::start_block_saver(Arc::clone(&self.miner), backlog_rx, self.kafka_connector.clone()),
                );

                let task_block_fetcher = spawn(
                    "importer::block-with-changes-fetcher",
                    Importer::start_block_with_changes_fetcher(Arc::clone(&self.chain), Arc::clone(storage), backlog_tx, number),
                );

                (task_saver, task_block_fetcher)
            }
        };

        let results = try_join!(exec_or_save_handle, block_fetcher_handle, task_number_fetcher)?;
        results.0?;
        results.1?;
        results.2?;

        Ok(())
    }

    // -----------------------------------------------------------------------------
    // Executor
    // -----------------------------------------------------------------------------

    pub const TASKS_COUNT: usize = 3;

    /// Receive data from channel with timeout
    async fn receive_with_timeout<T>(rx: &mut mpsc::Receiver<T>) -> Option<T> {
        match timeout(Duration::from_secs(2), rx.recv()).await {
            Ok(Some(inner)) => Some(inner),
            Ok(None) => None, // channel closed
            Err(_timed_out) => {
                tracing::warn!(timeout = "2s", "timeout reading block executor channel, expected around 1 block per second");
                None
            }
        }
    }

    /// Send block transactions to Kafka
    async fn send_block_to_kafka(kafka_connector: &Option<Arc<KafkaConnector>>, block: &Block) -> anyhow::Result<()> {
        if let Some(kafka_conn) = kafka_connector {
            let events = block
                .transactions
                .iter()
                .flat_map(|tx| transaction_to_events(block.header.timestamp, Cow::Borrowed(tx)));

            kafka_conn.send_buffered(events, 50).await?;
        }
        Ok(())
    }

    /// Record metrics for imported block
    #[cfg(feature = "metrics")]
    fn record_import_metrics(block_tx_len: usize, start: std::time::Instant) {
        metrics::inc_n_importer_online_transactions_total(block_tx_len as u64);
        metrics::inc_import_online_mined_block(start.elapsed());
    }

    // Executes external blocks and persist them to storage.
    async fn start_block_executor(
        executor: Arc<Executor>,
        miner: Arc<Miner>,
        mut backlog_rx: mpsc::Receiver<(ExternalBlock, Vec<ExternalReceipt>)>,
        kafka_connector: Option<Arc<KafkaConnector>>,
        importer_mode: ImporterMode,
    ) -> anyhow::Result<()> {
        const TASK_NAME: &str = "block-executor";
        let _permit = IMPORTER_ONLINE_TASKS_SEMAPHORE.acquire().await;

        loop {
            if Self::should_shutdown(TASK_NAME) {
                return Ok(());
            }

            let Some((block, receipts)) = Self::receive_with_timeout(&mut backlog_rx).await else {
                break;
            };

            #[cfg(feature = "metrics")]
            let (start, block_number, block_tx_len, receipts_len) = (metrics::now(), block.number(), block.transactions.len(), receipts.len());

            if let ImporterMode::FakeLeader = importer_mode {
                for tx in block.0.transactions.into_transactions() {
                    tracing::info!(?tx, "executing tx as fake miner");
                    if let Err(e) = executor.execute_local_transaction(tx.try_into()?) {
                        match e {
                            StratusError::Transaction(TransactionError::Nonce { transaction: _, account: _ }) => {
                                tracing::warn!(reason = ?e, "transaction failed, was this node restarted?");
                            }
                            _ => {
                                tracing::error!(reason = ?e, "transaction failed");
                                GlobalState::shutdown_from("Importer (FakeMiner)", "Transaction Failed");
                                bail!(e);
                            }
                        }
                    }
                }
                mine_and_commit(&miner);
                continue;
            }

            if let Err(e) = executor.execute_external_block(block.clone(), ExternalReceipts::from(receipts)) {
                let message = GlobalState::shutdown_from(TASK_NAME, "failed to reexecute external block");
                return log_and_err!(reason = e, message);
            };

            // statistics
            #[cfg(feature = "metrics")]
            {
                let duration = start.elapsed();
                let tps = calculate_tps(duration, block_tx_len);

                tracing::info!(
                    tps,
                    %block_number,
                    duration = %duration.to_string_ext(),
                    %receipts_len,
                    "reexecuted external block",
                );
            }

            let (mined_block, changes) = match miner.mine_external(block) {
                Ok((mined_block, changes)) => {
                    tracing::info!(number = %mined_block.number(), "mined external block");
                    (mined_block, changes)
                }
                Err(e) => {
                    let message = GlobalState::shutdown_from(TASK_NAME, "failed to mine external block");
                    return log_and_err!(reason = e, message);
                }
            };

            Self::send_block_to_kafka(&kafka_connector, &mined_block).await?;

            match miner.commit(CommitItem::Block(mined_block), changes) {
                Ok(_) => {
                    tracing::info!("committed external block");
                }
                Err(e) => {
                    let message = GlobalState::shutdown_from(TASK_NAME, "failed to commit external block");
                    return log_and_err!(reason = e, message);
                }
            }

            #[cfg(feature = "metrics")]
            Self::record_import_metrics(receipts_len, start);
        }

        warn_task_tx_closed(TASK_NAME);
        Ok(())
    }

    async fn start_block_saver(
        miner: Arc<Miner>,
        mut backlog_rx: mpsc::Receiver<(Block, ExecutionChanges)>,
        kafka_connector: Option<Arc<KafkaConnector>>,
    ) -> anyhow::Result<()> {
        const TASK_NAME: &str = "block-saver";
        let _permit = IMPORTER_ONLINE_TASKS_SEMAPHORE.acquire().await;

        loop {
            if Self::should_shutdown(TASK_NAME) {
                return Ok(());
            }

            let Some((block, changes)) = Self::receive_with_timeout(&mut backlog_rx).await else {
                break;
            };

            tracing::info!(block_number = %block.number(), "received block with changes");

            #[cfg(feature = "metrics")]
            let (start, block_tx_len) = (metrics::now(), block.transactions.len());

            Self::send_block_to_kafka(&kafka_connector, &block).await?;

            miner.commit(CommitItem::ReplicationBlock(block), changes)?;

            #[cfg(feature = "metrics")]
            Self::record_import_metrics(block_tx_len, start);
        }

        warn_task_tx_closed(TASK_NAME);
        Ok(())
    }

    // -----------------------------------------------------------------------------
    // Number fetcher
    // -----------------------------------------------------------------------------

    /// Retrieves the blockchain current block number.
    async fn start_number_fetcher(chain: Arc<BlockchainClient>, sync_interval: Duration) -> anyhow::Result<()> {
        const TASK_NAME: &str = "external-number-fetcher";
        let _permit = IMPORTER_ONLINE_TASKS_SEMAPHORE.acquire().await;

        // initial newHeads subscriptions.
        // abort application if cannot subscribe.
        let mut sub_new_heads = if chain.supports_ws() {
            tracing::info!("{} subscribing to newHeads event", TASK_NAME);

            match chain.subscribe_new_heads().await {
                Ok(sub) => {
                    tracing::info!("{} subscribed to newHeads events", TASK_NAME);
                    Some(sub)
                }
                Err(e) => {
                    let message = GlobalState::shutdown_from(TASK_NAME, "cannot subscribe to newHeads event");
                    return log_and_err!(reason = e, message);
                }
            }
        } else {
            tracing::warn!("{} blockchain client does not have websocket enabled", TASK_NAME);
            None
        };

        // keep reading websocket subscription or polling via http.
        loop {
            if Self::should_shutdown(TASK_NAME) {
                return Ok(());
            }

            // if we have a subscription, try to read from subscription.
            // in case of failure, re-subscribe because current subscription may have been closed in the server.
            if let Some(sub) = &mut sub_new_heads {
                tracing::info!("{} awaiting block number from newHeads subscription", TASK_NAME);
                match timeout(TIMEOUT_NEW_HEADS, sub.next()).await {
                    Ok(Some(Ok(block))) => {
                        tracing::info!(block_number = %block.number(), "{} received newHeads event", TASK_NAME);
                        set_external_rpc_current_block(block.number());
                        continue;
                    }
                    Ok(None) =>
                        if !Self::should_shutdown(TASK_NAME) {
                            tracing::error!("{} newHeads subscription closed by the other side", TASK_NAME);
                        },
                    Ok(Some(Err(e))) =>
                        if !Self::should_shutdown(TASK_NAME) {
                            tracing::error!(reason = ?e, "{} failed to read newHeads subscription event", TASK_NAME);
                        },
                    Err(_) =>
                        if !Self::should_shutdown(TASK_NAME) {
                            tracing::error!("{} timed-out waiting for newHeads subscription event", TASK_NAME);
                        },
                }

                if Self::should_shutdown(TASK_NAME) {
                    return Ok(());
                }

                // resubscribe if necessary.
                // only update the existing subscription if succedeed, otherwise we will try again in the next iteration.
                if chain.supports_ws() {
                    tracing::info!("{} resubscribing to newHeads event", TASK_NAME);
                    match chain.subscribe_new_heads().await {
                        Ok(sub) => {
                            tracing::info!("{} resubscribed to newHeads event", TASK_NAME);
                            sub_new_heads = Some(sub);
                        }
                        Err(e) =>
                            if !Self::should_shutdown(TASK_NAME) {
                                tracing::error!(reason = ?e, "{} failed to resubscribe to newHeads event", TASK_NAME);
                            },
                    }
                }
            }

            if Self::should_shutdown(TASK_NAME) {
                return Ok(());
            }

            // fallback to polling
            tracing::warn!("{} falling back to http polling because subscription failed or it is not enabled", TASK_NAME);
            match chain.fetch_block_number().await {
                Ok(block_number) => {
                    tracing::info!(
                        %block_number,
                        sync_interval = %sync_interval.to_string_ext(),
                        "fetched current block number via http. awaiting sync interval to retrieve again."
                    );
                    set_external_rpc_current_block(block_number);
                    traced_sleep(sync_interval, SleepReason::SyncData).await;
                }
                Err(e) =>
                    if !Self::should_shutdown(TASK_NAME) {
                        tracing::error!(reason = ?e, "failed to retrieve block number. retrying now.");
                    },
            }
        }
    }

    // -----------------------------------------------------------------------------
    // Block fetcher
    // -----------------------------------------------------------------------------

    /// Generic block fetcher that handles the common logic of fetching blocks in parallel
    async fn generic_block_fetcher<T, U, FetchFut, ProcessFut, FetchFn, ProcessFn>(
        task_name: &'static str,
        backlog_tx: mpsc::Sender<U>,
        mut importer_block_number: BlockNumber,
        fetch_fn: FetchFn,
        process_fn: ProcessFn,
    ) -> anyhow::Result<()>
    where
        FetchFut: std::future::Future<Output = T>,
        ProcessFut: std::future::Future<Output = anyhow::Result<U>>,
        FetchFn: Fn(BlockNumber) -> FetchFut,
        ProcessFn: Fn(T) -> ProcessFut,
    {
        let _permit = IMPORTER_ONLINE_TASKS_SEMAPHORE.acquire().await;

        loop {
            if Self::should_shutdown(task_name) {
                return Ok(());
            }

            // if we are ahead of current block number, await until we are behind again
            let external_rpc_current_block = EXTERNAL_RPC_CURRENT_BLOCK.load(Ordering::Relaxed);
            if importer_block_number.as_u64() > external_rpc_current_block {
                yield_now().await;
                continue;
            }

            // we are behind current, so we will fetch multiple blocks in parallel to catch up
            let blocks_behind = importer_block_number.count_to(external_rpc_current_block);
            let mut blocks_to_fetch = min(blocks_behind, 1_000); // avoid spawning millions of tasks (not parallelism), at least until we know it is safe
            tracing::info!(%blocks_behind, blocks_to_fetch, "catching up with blocks");

            let mut tasks = Vec::with_capacity(blocks_to_fetch as usize);
            while blocks_to_fetch > 0 {
                blocks_to_fetch -= 1;
                tasks.push(fetch_fn(importer_block_number));
                importer_block_number = importer_block_number.next_block_number();
            }

            // keep fetching in order
            let mut tasks = futures::stream::iter(tasks).buffered(PARALLEL_BLOCKS);
            while let Some(fetched_data) = tasks.next().await {
                let processed = process_fn(fetched_data).await?;
                if backlog_tx.send(processed).await.is_err() {
                    warn_task_rx_closed(task_name);
                    return Ok(());
                }
            }
        }
    }

    /// Retrieves blocks and receipts.
    async fn start_block_fetcher(
        chain: Arc<BlockchainClient>,
        backlog_tx: mpsc::Sender<(ExternalBlock, Vec<ExternalReceipt>)>,
        importer_block_number: BlockNumber,
    ) -> anyhow::Result<()> {
        let fetch_fn = |block_number| fetch_block_and_receipts(Arc::clone(&chain), block_number);
        let process_fn = |data: (ExternalBlock, Vec<ExternalReceipt>)| async move {
            let (mut block, mut receipts) = data;
            let block_number = block.number();
            let BlockTransactions::Full(transactions) = &mut block.transactions else {
                bail!("expected full transactions, got hashes or uncle");
            };

            if transactions.len() != receipts.len() {
                bail!(
                    "block {} has mismatched transaction and receipt length: {} transactions but {} receipts",
                    block_number,
                    transactions.len(),
                    receipts.len()
                );
            }

            // Stably sort transactions and receipts by transaction_index
            transactions.sort_by(|a, b| a.transaction_index.cmp(&b.transaction_index));
            receipts.sort_by(|a, b| a.transaction_index.cmp(&b.transaction_index));

            // perform additional checks on the transaction index
            for window in transactions.windows(2) {
                let tx_index = window[0].transaction_index.ok_or(anyhow!("missing transaction index"))? as u32;
                let next_tx_index = window[1].transaction_index.ok_or(anyhow!("missing transaction index"))? as u32;
                if tx_index + 1 != next_tx_index {
                    tracing::error!(tx_index, next_tx_index, "two consecutive transactions must have consecutive indices");
                }
            }
            for window in receipts.windows(2) {
                let tx_index = window[0].transaction_index.ok_or(anyhow!("missing transaction index"))? as u32;
                let next_tx_index = window[1].transaction_index.ok_or(anyhow!("missing transaction index"))? as u32;
                if tx_index + 1 != next_tx_index {
                    tracing::error!(tx_index, next_tx_index, "two consecutive receipts must have consecutive indices");
                }
            }

            Ok((block, receipts))
        };

        Self::generic_block_fetcher("external-block-fetcher", backlog_tx, importer_block_number, fetch_fn, process_fn).await
    }

    /// Retrieves blocks with changes.
    async fn start_block_with_changes_fetcher(
        chain: Arc<BlockchainClient>,
        storage: Arc<StratusStorage>,
        backlog_tx: mpsc::Sender<(Block, ExecutionChanges)>,
        importer_block_number: BlockNumber,
    ) -> anyhow::Result<()> {
        let fetch_fn = |block_number| fetch_block_with_changes(Arc::clone(&chain), block_number);
        let process_fn = |data| {
            let storage = Arc::clone(&storage);
            async move {
                let (block, changes) = data;
                let changes = create_execution_changes(&storage, changes)?;
                Ok((block, changes))
            }
        };
        Self::generic_block_fetcher("block-with-changes-fetcher", backlog_tx, importer_block_number, fetch_fn, process_fn).await
    }
}

fn create_execution_changes(storage: &Arc<StratusStorage>, changes: BlockChangesRocksdb) -> anyhow::Result<ExecutionChanges> {
    let addresses = changes.account_changes.keys().copied().map_into().collect_vec();
    let accounts = storage.perm.read_accounts(addresses)?;
    let mut exec_changes = changes.to_incomplete_execution_changes();
    for (addr, acc) in accounts {
        match exec_changes.accounts.entry(addr) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let item = entry.get_mut();
                item.apply_original(acc);
            }
            std::collections::hash_map::Entry::Vacant(_) => unreachable!("we got the addresses from the changes"),
        }
    }
    Ok(exec_changes)
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

/// Generic retry logic for fetching data from blockchain
async fn fetch_with_retry<T, F, Fut>(block_number: BlockNumber, fetch_fn: F, operation_name: &str) -> T
where
    F: Fn(BlockNumber) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Option<T>>>,
{
    const RETRY_DELAY: Duration = Duration::from_millis(10);
    Span::with(|s| {
        s.rec_str("block_number", &block_number);
    });

    loop {
        tracing::info!(%block_number, "fetching {}", operation_name);

        match fetch_fn(block_number).await {
            Ok(Some(response)) => return response,
            Ok(None) => {
                tracing::warn!(
                    %block_number,
                    delay_ms = %RETRY_DELAY.as_millis(),
                    "{} not available yet, retrying with delay.",
                    operation_name
                );
                traced_sleep(RETRY_DELAY, SleepReason::RetryBackoff).await;
            }
            Err(e) => {
                tracing::warn!(
                    reason = ?e,
                    %block_number,
                    delay_ms = %RETRY_DELAY.as_millis(),
                    "failed to fetch {}, retrying with delay.",
                    operation_name
                );
                traced_sleep(RETRY_DELAY, SleepReason::RetryBackoff).await;
            }
        };
    }
}

async fn fetch_block_and_receipts(chain: Arc<BlockchainClient>, block_number: BlockNumber) -> (ExternalBlock, Vec<ExternalReceipt>) {
    let fetch_fn = |bn| {
        let chain = Arc::clone(&chain);
        async move {
            chain
                .fetch_block_and_receipts(bn)
                .await
                .map(|opt| opt.map(|response| (response.block, response.receipts)))
        }
    };

    fetch_with_retry(block_number, fetch_fn, "block and receipts").await
}

async fn fetch_block_with_changes(chain: Arc<BlockchainClient>, block_number: BlockNumber) -> (Block, BlockChangesRocksdb) {
    let fetch_fn = |bn| chain.fetch_block_with_changes(bn);
    fetch_with_retry(block_number, fetch_fn, "block and changes").await
}

impl Consensus for Importer {
    async fn lag(&self) -> anyhow::Result<u64> {
        let last_fetched_time = LATEST_FETCHED_BLOCK_TIME.load(Ordering::Relaxed);

        if last_fetched_time == 0 {
            bail!("stratus has not been able to connect to the leader yet");
        }

        let elapsed = chrono::Utc::now().timestamp() as u64 - last_fetched_time;
        if elapsed > 4 {
            Err(anyhow::anyhow!(
                "too much time elapsed without communicating with the leader. elapsed: {elapsed}s"
            ))
        } else {
            Ok(EXTERNAL_RPC_CURRENT_BLOCK.load(Ordering::SeqCst) - self.storage.read_mined_block_number().as_u64())
        }
    }

    fn get_chain(&self) -> anyhow::Result<&Arc<BlockchainClient>> {
        Ok(&self.chain)
    }
}
