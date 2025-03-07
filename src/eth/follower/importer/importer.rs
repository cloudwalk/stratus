use std::borrow::Cow;
use std::cmp::min;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use alloy_rpc_types_eth::BlockTransactions;
use anyhow::anyhow;
use futures::try_join;
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio::task::yield_now;
use tokio::time::timeout;
use tracing::Span;

use crate::eth::executor::Executor;
use crate::eth::follower::consensus::Consensus;
use crate::eth::miner::miner::interval_miner::mine_and_commit;
use crate::eth::miner::Miner;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalReceipts;
use crate::eth::storage::StratusStorage;
use crate::ext::spawn_named;
use crate::ext::traced_sleep;
use crate::ext::DisplayExt;
use crate::ext::SleepReason;
use crate::globals::IMPORTER_ONLINE_TASKS_SEMAPHORE;
use crate::infra::kafka::KafkaConnector;
#[cfg(feature = "metrics")]
use crate::infra::metrics;
use crate::infra::tracing::warn_task_rx_closed;
use crate::infra::tracing::warn_task_tx_closed;
use crate::infra::tracing::SpanExt;
use crate::infra::BlockchainClient;
use crate::ledger::events::transaction_to_events;
use crate::log_and_err;
#[cfg(feature = "metrics")]
use crate::utils::calculate_tps;
use crate::utils::DropTimer;
use crate::GlobalState;

#[derive(Clone, Copy)]
pub enum ImporterMode {
    /// A normal follower imports a mined block.
    NormalFollower,
    /// Fake leader feches a block, re-executes its txs and then mines it's own block.
    FakeLeader,
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
        let number = storage.read_block_number_to_resume_import()?;

        let (backlog_tx, backlog_rx) = mpsc::unbounded_channel();

        // spawn block executor:
        // it executes and mines blocks and expects to receive them via channel in the correct order.
        let task_executor = spawn_named(
            "importer::executor",
            Importer::start_block_executor(
                Arc::clone(&self.executor),
                Arc::clone(&self.miner),
                backlog_rx,
                self.kafka_connector.clone(),
                self.importer_mode,
            ),
        );

        // spawn block number:
        // it keeps track of the blockchain current block number.
        let number_fetcher_chain = Arc::clone(&self.chain);
        let task_number_fetcher = spawn_named(
            "importer::number-fetcher",
            Importer::start_number_fetcher(number_fetcher_chain, self.sync_interval),
        );

        // spawn block fetcher:
        // it fetches blocks and receipts in parallel and sends them to the executor in the correct order.
        // it uses the number fetcher current block to determine if should keep downloading more blocks or not.
        let block_fetcher_chain = Arc::clone(&self.chain);
        let task_block_fetcher = spawn_named(
            "importer::block-fetcher",
            Importer::start_block_fetcher(block_fetcher_chain, backlog_tx, number),
        );

        // await all tasks
        if let Err(e) = try_join!(task_executor, task_block_fetcher, task_number_fetcher) {
            tracing::error!(reason = ?e, "importer-online failed");
        }
        Ok(())
    }

    // -----------------------------------------------------------------------------
    // Executor
    // -----------------------------------------------------------------------------

    pub const TASKS_COUNT: usize = 3;

    // Executes external blocks and persist them to storage.
    async fn start_block_executor(
        executor: Arc<Executor>,
        miner: Arc<Miner>,
        mut backlog_rx: mpsc::UnboundedReceiver<(ExternalBlock, Vec<ExternalReceipt>)>,
        kafka_connector: Option<Arc<KafkaConnector>>,
        importer_mode: ImporterMode,
    ) -> anyhow::Result<()> {
        const TASK_NAME: &str = "block-executor";
        let _permit = IMPORTER_ONLINE_TASKS_SEMAPHORE.acquire().await;

        loop {
            if Self::should_shutdown(TASK_NAME) {
                return Ok(());
            }

            let (block, receipts) = match timeout(Duration::from_secs(2), backlog_rx.recv()).await {
                Ok(Some(inner)) => inner,
                Ok(None) => break, // channel closed
                Err(_timed_out) => {
                    tracing::warn!(timeout = "2s", "timeout reading block executor channel, expected around 1 block per second");
                    continue;
                }
            };

            #[cfg(feature = "metrics")]
            let (start, block_number, block_tx_len, receipts_len) = (metrics::now(), block.number(), block.transactions.len(), receipts.len());

            if let ImporterMode::FakeLeader = importer_mode {
                for tx in block.0.transactions.into_transactions() {
                    tracing::info!(?tx, "executing tx as fake miner");
                    if let Err(e) = executor.execute_local_transaction(tx.try_into()?) {
                        tracing::error!(reason = ?e, "transaction failed");
                        GlobalState::shutdown_from("Importer (FakeMiner)", "Transaction Failed");
                        return Err(anyhow!(e));
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

            let mined_block = match miner.mine_external(block) {
                Ok(mined_block) => {
                    tracing::info!(number = %mined_block.number(), "mined external block");
                    mined_block
                }
                Err(e) => {
                    let message = GlobalState::shutdown_from(TASK_NAME, "failed to mine external block");
                    return log_and_err!(reason = e, message);
                }
            };

            if let Some(ref kafka_conn) = kafka_connector {
                let events = mined_block
                    .transactions
                    .iter()
                    .flat_map(|tx| transaction_to_events(mined_block.header.timestamp, Cow::Borrowed(tx)));

                kafka_conn.send_buffered(events, 50).await?;
            }

            match miner.commit(mined_block) {
                Ok(_) => {
                    tracing::info!("committed external block");
                }
                Err(e) => {
                    let message = GlobalState::shutdown_from(TASK_NAME, "failed to commit external block");
                    return log_and_err!(reason = e, message);
                }
            }

            #[cfg(feature = "metrics")]
            {
                metrics::inc_n_importer_online_transactions_total(receipts_len as u64);
                metrics::inc_import_online_mined_block(start.elapsed());
            }
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

    /// Retrieves blocks and receipts.
    async fn start_block_fetcher(
        chain: Arc<BlockchainClient>,
        backlog_tx: mpsc::UnboundedSender<(ExternalBlock, Vec<ExternalReceipt>)>,
        mut importer_block_number: BlockNumber,
    ) -> anyhow::Result<()> {
        const TASK_NAME: &str = "external-block-fetcher";
        let _permit = IMPORTER_ONLINE_TASKS_SEMAPHORE.acquire().await;

        loop {
            if Self::should_shutdown(TASK_NAME) {
                return Ok(());
            }

            // if we are ahead of current block number, await until we are behind again
            let external_rpc_current_block = EXTERNAL_RPC_CURRENT_BLOCK.load(Ordering::Relaxed);
            if importer_block_number.as_u64() > external_rpc_current_block {
                yield_now().await;
                continue;
            }

            // we are behind current, so we will fetch multiple blocks in parallel to catch up
            let blocks_behind = external_rpc_current_block.saturating_sub(importer_block_number.as_u64()) + 1; // TODO: use count_to from BlockNumber
            let mut blocks_to_fetch = min(blocks_behind, 1_000); // avoid spawning millions of tasks (not parallelism), at least until we know it is safe
            tracing::info!(%blocks_behind, blocks_to_fetch, "catching up with blocks");

            let mut tasks = Vec::with_capacity(blocks_to_fetch as usize);
            while blocks_to_fetch > 0 {
                blocks_to_fetch -= 1;
                tasks.push(fetch_block_and_receipts(Arc::clone(&chain), importer_block_number));
                importer_block_number = importer_block_number.next_block_number();
            }

            // keep fetching in order
            let mut tasks = futures::stream::iter(tasks).buffered(PARALLEL_BLOCKS);
            while let Some((mut block, mut receipts)) = tasks.next().await {
                let block_number = block.number();
                let BlockTransactions::Full(transactions) = &mut block.transactions else {
                    return Err(anyhow!("expected full transactions, got hashes or uncle"));
                };

                if transactions.len() != receipts.len() {
                    return Err(anyhow!(
                        "block {} has mismatched transaction and receipt length: {} transactions but {} receipts",
                        block_number,
                        transactions.len(),
                        receipts.len()
                    ));
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

                if backlog_tx.send((block, receipts)).is_err() {
                    warn_task_rx_closed(TASK_NAME);
                    return Ok(());
                }
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------
async fn fetch_block_and_receipts(chain: Arc<BlockchainClient>, block_number: BlockNumber) -> (ExternalBlock, Vec<ExternalReceipt>) {
    const RETRY_DELAY: Duration = Duration::from_millis(10);
    Span::with(|s| {
        s.rec_str("block_number", &block_number);
    });

    loop {
        tracing::info!(%block_number, "fetching block and receipts");

        match chain.fetch_block_and_receipts(block_number).await {
            Ok(Some(response)) => return (response.block, response.receipts),
            Ok(None) => {
                tracing::warn!(%block_number, delay_ms = %RETRY_DELAY.as_millis(), "block and receipts not available yet, retrying with delay.");
                traced_sleep(RETRY_DELAY, SleepReason::RetryBackoff).await;
            }
            Err(e) => {
                tracing::warn!(reason = ?e, %block_number, delay_ms = %RETRY_DELAY.as_millis(), "failed to fetch block and receipts, retrying with delay.");
                traced_sleep(RETRY_DELAY, SleepReason::RetryBackoff).await;
            }
        };
    }
}

async fn fetch_raw_block(chain: Arc<BlockchainClient>, block_number: BlockNumber) -> anyhow::Result<Block> {
    const RETRY_DELAY: Duration = Duration::from_millis(10);
    Span::with(|s| {
        s.rec_str("block_number", &block_number);
    });

    loop {
        tracing::info!(%block_number, "fetching raw block");

        match chain.fetch_raw_block(block_number).await {
            Ok(Some(raw_block)) => {
                return Ok(raw_block);
            }
            Ok(None) => {
                tracing::warn!(%block_number, delay_ms = %RETRY_DELAY.as_millis(), "raw block not available yet, retrying with delay.");
                traced_sleep(RETRY_DELAY, SleepReason::RetryBackoff).await;
            }
            Err(e) => {
                tracing::warn!(reason = ?e, %block_number, delay_ms = %RETRY_DELAY.as_millis(), "failed to fetch raw block, retrying with delay.");
                traced_sleep(RETRY_DELAY, SleepReason::RetryBackoff).await;
            }
        }
    }
}

impl Consensus for Importer {
    async fn lag(&self) -> anyhow::Result<u64> {
        let elapsed = chrono::Utc::now().timestamp() as u64 - LATEST_FETCHED_BLOCK_TIME.load(Ordering::Relaxed);
        if elapsed > 4 {
            Err(anyhow::anyhow!(
                "too much time elapsed without communicating with the leader. elapsed: {}s",
                elapsed
            ))
        } else {
            Ok(EXTERNAL_RPC_CURRENT_BLOCK.load(Ordering::SeqCst) - self.storage.read_mined_block_number()?.as_u64())
        }
    }

    fn get_chain(&self) -> anyhow::Result<&Arc<BlockchainClient>> {
        Ok(&self.chain)
    }
}
