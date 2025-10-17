use std::borrow::Cow;
use std::cmp::min;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use alloy_rpc_types_eth::BlockTransactions;
use anyhow::anyhow;
use anyhow::bail;
use async_trait::async_trait;
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

pub struct OldImporter {
    executor: Arc<Executor>,
    miner: Arc<Miner>,
    storage: Arc<StratusStorage>,
    chain: Arc<BlockchainClient>,
    sync_interval: Duration,
    kafka_connector: Option<Arc<KafkaConnector>>,
    importer_mode: ImporterMode,
}

impl OldImporter {
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

    // -----------------------------------------------------------------------------
    // Executor
    // -----------------------------------------------------------------------------

    pub const TASKS_COUNT: usize = 3;

    /// Receive data from channel with timeout
    async fn receive_with_timeout<T>(rx: &mut mpsc::Receiver<T>) -> anyhow::Result<Option<T>> {
        match timeout(Duration::from_secs(2), rx.recv()).await {
            Ok(Some(inner)) => Ok(Some(inner)),
            Ok(None) => bail!("channel closed"),
            Err(_timed_out) => {
                tracing::warn!(timeout = "2s", "timeout reading block executor channel, expected around 1 block per second");
                Ok(None)
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
                    Ok(None) => {
                        if !Self::should_shutdown(TASK_NAME) {
                            tracing::error!("{} newHeads subscription closed by the other side", TASK_NAME);
                        }
                    }
                    Ok(Some(Err(e))) => {
                        if !Self::should_shutdown(TASK_NAME) {
                            tracing::error!(reason = ?e, "{} failed to read newHeads subscription event", TASK_NAME);
                        }
                    }
                    Err(_) => {
                        if !Self::should_shutdown(TASK_NAME) {
                            tracing::error!("{} timed-out waiting for newHeads subscription event", TASK_NAME);
                        }
                    }
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
                        Err(e) => {
                            if !Self::should_shutdown(TASK_NAME) {
                                tracing::error!(reason = ?e, "{} failed to resubscribe to newHeads event", TASK_NAME);
                            }
                        }
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
                Err(e) => {
                    if !Self::should_shutdown(TASK_NAME) {
                        tracing::error!(reason = ?e, "failed to retrieve block number. retrying now.");
                    }
                }
            }
        }
    }

    fn should_shutdown(task_name: &str) -> bool {
        GlobalState::is_shutdown_warn(task_name) || GlobalState::is_importer_shutdown_warn(task_name)
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

struct BlockChangesFetcherWorker {
    chain: Arc<BlockchainClient>,
    storage: Arc<StratusStorage>,
}

#[async_trait]
impl FetcherWorker<(Block, BlockChangesRocksdb), (Block, ExecutionChanges)> for BlockChangesFetcherWorker {
    async fn fetch(&self, block_number: BlockNumber) -> (Block, BlockChangesRocksdb) {
        let fetch_fn = |bn| self.chain.fetch_block_with_changes(bn);
        fetch_with_retry(block_number, fetch_fn, "block and changes").await
    }

    async fn post_process(&self, data: (Block, BlockChangesRocksdb)) -> anyhow::Result<(Block, ExecutionChanges)> {
        let storage = Arc::clone(&self.storage);
        let (block, changes) = data;
        let changes = create_execution_changes(&storage, changes)?;
        Ok((block, changes))
    }
}

struct ExecutionFetcherWorker {
    chain: Arc<BlockchainClient>,
}

#[async_trait]
impl FetcherWorker<(ExternalBlock, Vec<ExternalReceipt>), (ExternalBlock, Vec<ExternalReceipt>)> for ExecutionFetcherWorker {
    async fn fetch(&self, block_number: BlockNumber) -> (ExternalBlock, Vec<ExternalReceipt>) {
        let fetch_fn = |bn| {
            let chain = Arc::clone(&self.chain);
            async move {
                chain
                    .fetch_block_and_receipts(bn)
                    .await
                    .map(|opt| opt.map(|response| (response.block, response.receipts)))
            }
        };

        fetch_with_retry(block_number, fetch_fn, "block and receipts").await
    }

    async fn post_process(&self, data: (ExternalBlock, Vec<ExternalReceipt>)) -> anyhow::Result<(ExternalBlock, Vec<ExternalReceipt>)> {
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
    }
}

#[async_trait]
trait FetcherWorker<FetchedType: Send, PostProcessType: Send>: Send + Sync {
    async fn fetch(&self, block_number: BlockNumber) -> FetchedType;
    async fn post_process(&self, data: FetchedType) -> anyhow::Result<PostProcessType>;

    /// Generic block fetcher that handles the common logic of fetching blocks in parallel
    async fn run(self, backlog_tx: mpsc::Sender<PostProcessType>, mut importer_block_number: BlockNumber) -> anyhow::Result<()> {
        let _permit = IMPORTER_ONLINE_TASKS_SEMAPHORE.acquire().await;
        const TASK_NAME: &str = "importer block fetcher";

        loop {
            if OldImporter::should_shutdown(TASK_NAME) {
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
                tasks.push(self.fetch(importer_block_number));
                importer_block_number = importer_block_number.next_block_number();
            }

            // keep fetching in order
            let mut tasks = futures::stream::iter(tasks).buffered(PARALLEL_BLOCKS);
            while let Some(fetched_data) = tasks.next().await {
                let processed = self.post_process(fetched_data).await?;
                if backlog_tx.send(processed).await.is_err() {
                    warn_task_rx_closed(TASK_NAME);
                    return Ok(());
                }
            }
        }
    }
}

struct FakeLeaderWorker {
    executor: Arc<Executor>,
    miner: Arc<Miner>,
}

#[async_trait]
impl ImporterWorker<(ExternalBlock, Vec<ExternalReceipt>)> for FakeLeaderWorker {
    async fn import(&self, (block, _): (ExternalBlock, Vec<ExternalReceipt>)) -> anyhow::Result<()> {
        for tx in block.0.transactions.into_transactions() {
            tracing::info!(?tx, "executing tx as fake miner");
            if let Err(e) = self.executor.execute_local_transaction(tx.try_into()?) {
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
        mine_and_commit(&self.miner);
        Ok(())
    }
}

struct BlockExecutorWorker {
    executor: Arc<Executor>,
    miner: Arc<Miner>,
    kafka_connector: Option<Arc<KafkaConnector>>,
}

#[async_trait]
impl ImporterWorker<(ExternalBlock, Vec<ExternalReceipt>)> for BlockExecutorWorker {
    async fn import(&self, (block, receipts): (ExternalBlock, Vec<ExternalReceipt>)) -> anyhow::Result<()> {
        const TASK_NAME: &str = "block-executor";

        #[cfg(feature = "metrics")]
        let (start, block_number, block_tx_len, receipts_len) = (metrics::now(), block.number(), block.transactions.len(), receipts.len());

        if let Err(e) = self.executor.execute_external_block(block.clone(), ExternalReceipts::from(receipts)) {
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

        let (mined_block, changes) = match self.miner.mine_external(block) {
            Ok((mined_block, changes)) => {
                tracing::info!(number = %mined_block.number(), "mined external block");
                (mined_block, changes)
            }
            Err(e) => {
                let message = GlobalState::shutdown_from(TASK_NAME, "failed to mine external block");
                return log_and_err!(reason = e, message);
            }
        };

        OldImporter::send_block_to_kafka(&self.kafka_connector, &mined_block).await?;

        match self.miner.commit(CommitItem::Block(mined_block), changes) {
            Ok(_) => {
                tracing::info!("committed external block");
            }
            Err(e) => {
                let message = GlobalState::shutdown_from(TASK_NAME, "failed to commit external block");
                return log_and_err!(reason = e, message);
            }
        }

        #[cfg(feature = "metrics")]
        OldImporter::record_import_metrics(receipts_len, start);

        Ok(())
    }
}

struct BlockSaverWorker {
    miner: Arc<Miner>,
    kafka_connector: Option<Arc<KafkaConnector>>,
}


#[async_trait]
impl ImporterWorker<(Block, ExecutionChanges)> for BlockSaverWorker {
    async fn import(&self, (block, changes): (Block, ExecutionChanges)) -> anyhow::Result<()> {
        tracing::info!(block_number = %block.number(), "received block with changes");

        #[cfg(feature = "metrics")]
        let (start, block_tx_len) = (metrics::now(), block.transactions.len());

        OldImporter::send_block_to_kafka(&self.kafka_connector, &block).await?;

        self.miner.commit(CommitItem::ReplicationBlock(block), changes)?;

        #[cfg(feature = "metrics")]
        OldImporter::record_import_metrics(block_tx_len, start);

        Ok(())
    }
}

#[async_trait]
trait ImporterWorker<DataType: Send>: Send + Sync {
    async fn import(&self, data: DataType) -> anyhow::Result<()>;
    async fn run(self, mut backlog_rx: mpsc::Receiver<DataType>) -> anyhow::Result<()> {
        const TASK_NAME: &str = "importer-worker";
        let _permit = IMPORTER_ONLINE_TASKS_SEMAPHORE.acquire().await;
        loop {
            if OldImporter::should_shutdown(TASK_NAME) {
                return Ok(());
            }

            let data = match OldImporter::receive_with_timeout(&mut backlog_rx).await {
                Ok(Some(inner)) => inner,
                Ok(None) => continue,
                Err(_) => break,
            };

            self.import(data).await?;
        }

        warn_task_tx_closed(TASK_NAME);
        Ok(())
    }
}

struct ImporterSupervisor<Fetcher: FetcherWorker<FT, PT>, Importer: ImporterWorker<PT>, FT: Send, PT: Send> {
    fetcher: Fetcher,
    importer: Importer,
    _phantom: std::marker::PhantomData<(FT, PT)>,
}

impl<Fetcher: FetcherWorker<FT, PT>, Importer: ImporterWorker<PT>, FT: Send, PT: Send> ImporterSupervisor<Fetcher, Importer, FT, PT> {
    pub async fn run(self, resume_from: BlockNumber, sync_interval: Duration, chain: Arc<BlockchainClient>) -> anyhow::Result<()> {
        let _timer = DropTimer::start("importer-online::run_importer_online");

        // Spawn common tasks: number fetcher
        let number_fetcher_task = spawn("importer::number-fetcher", OldImporter::start_number_fetcher(Arc::clone(&chain), sync_interval));

        let (backlog_tx, backlog_rx) = mpsc::channel(10_000);
        let importer_task = spawn("importer::importer", self.importer.run(backlog_rx));

        let fetcher_task = spawn("importer::fetcher", self.fetcher.run(backlog_tx, resume_from));

        let results = try_join!(importer_task, fetcher_task, number_fetcher_task)?;
        results.0?;
        results.1?;
        results.2?;

        Ok(())
    }
}

impl Consensus for OldImporter {
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
