use std::cmp::min;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use futures::try_join;
use futures::StreamExt;
use serde::Deserialize;
use stratus::channel_read;
use stratus::config::ImporterOnlineConfig;
use stratus::eth::primitives::BlockNumber;
use stratus::eth::primitives::ExternalBlock;
use stratus::eth::primitives::ExternalReceipt;
use stratus::eth::primitives::ExternalReceipts;
use stratus::eth::primitives::Hash;
use stratus::eth::storage::StratusStorage;
use stratus::eth::BlockMiner;
use stratus::eth::Executor;
use stratus::ext::named_spawn;
use stratus::ext::DisplayExt;
use stratus::if_else;
#[cfg(feature = "metrics")]
use stratus::infra::metrics;
use stratus::infra::tracing::warn_task_rx_closed;
use stratus::infra::tracing::warn_task_tx_closed;
use stratus::infra::BlockchainClient;
use stratus::log_and_err;
#[cfg(feature = "metrics")]
use stratus::utils::calculate_tps;
use stratus::GlobalServices;
use stratus::GlobalState;
use tokio::sync::mpsc;
use tokio::task::yield_now;
use tokio::time::sleep;
use tokio::time::timeout;

// -----------------------------------------------------------------------------
// Globals
// -----------------------------------------------------------------------------

/// Current block number of the external RPC blockchain.
static EXTERNAL_RPC_CURRENT_BLOCK: AtomicU64 = AtomicU64::new(0);

/// Only sets the external RPC current block number if it is equals or greater than the current one.
fn set_external_rpc_current_block(new_number: BlockNumber) {
    let new_number_u64 = new_number.as_u64();
    let _ = EXTERNAL_RPC_CURRENT_BLOCK.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current_number| {
        if_else!(new_number_u64 >= current_number, Some(new_number_u64), None)
    });
}

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------
/// Number of blocks that are downloaded in parallel.
const PARALLEL_BLOCKS: usize = 3;

/// Number of receipts that are downloaded in parallel.
const PARALLEL_RECEIPTS: usize = 100;

/// Timeout awaiting for newHeads event before fallback to polling.
const TIMEOUT_NEW_HEADS: Duration = Duration::from_millis(2000);

/// Interval before we starting retrieving receipts because they are not immediately available after the block is retrieved.
const INTERVAL_FETCH_RECEIPTS: Duration = Duration::from_millis(50);

// -----------------------------------------------------------------------------
// Execution
// -----------------------------------------------------------------------------
#[allow(dead_code)]
fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<ImporterOnlineConfig>::init()?;
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: ImporterOnlineConfig) -> anyhow::Result<()> {
    // init server
    let storage = config.storage.init().await?;
    let relayer = config.relayer.init().await?;
    let miner = config.miner.init_external_mode(Arc::clone(&storage), None, None).await?;
    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner), relayer, None).await; //XXX TODO implement the consensus here, in case of it being a follower, it should not even enter here
    let chain = Arc::new(
        BlockchainClient::new_http_ws(
            &config.base.external_rpc,
            config.base.external_rpc_ws.as_deref(),
            config.base.external_rpc_timeout,
        )
        .await?,
    );

    let result = run_importer_online(executor, miner, storage, chain, config.base.sync_interval).await;
    if let Err(ref e) = result {
        tracing::error!(reason = ?e, "importer-online failed");
    }
    result
}

pub async fn run_importer_online(
    executor: Arc<Executor>,
    miner: Arc<BlockMiner>,
    storage: Arc<StratusStorage>,
    chain: Arc<BlockchainClient>,
    sync_interval: Duration,
) -> anyhow::Result<()> {
    let number = storage.read_block_number_to_resume_import().await?;

    let (backlog_tx, backlog_rx) = mpsc::unbounded_channel();

    // spawn block executor:
    // it executes and mines blocks and expects to receive them via channel in the correct order.
    let task_executor = named_spawn("importer::executor", start_block_executor(executor, miner, backlog_rx));

    // spawn block number:
    // it keeps track of the blockchain current block number.
    let number_fetcher_chain = Arc::clone(&chain);
    let task_number_fetcher = named_spawn("importer::number-fetcher", start_number_fetcher(number_fetcher_chain, sync_interval));

    // spawn block fetcher:
    // it fetches blocks and receipts in parallel and sends them to the executor in the correct order.
    // it uses the number fetcher current block to determine if should keep downloading more blocks or not.
    let block_fetcher_chain = Arc::clone(&chain);
    let task_block_fetcher = named_spawn("importer::block-fetcher", start_block_fetcher(block_fetcher_chain, backlog_tx, number));

    // await all tasks
    if let Err(e) = try_join!(task_executor, task_block_fetcher, task_number_fetcher) {
        tracing::error!(reason = ?e, "importer-online failed");
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// Executor
// -----------------------------------------------------------------------------

// Executes external blocks and persist them to storage.
async fn start_block_executor(
    executor: Arc<Executor>,
    miner: Arc<BlockMiner>,
    mut backlog_rx: mpsc::UnboundedReceiver<(ExternalBlock, Vec<ExternalReceipt>)>,
) -> anyhow::Result<()> {
    const TASK_NAME: &str = "block-executor";

    while let Some((block, receipts)) = channel_read!(backlog_rx) {
        if GlobalState::warn_if_shutdown(TASK_NAME) {
            return Ok(());
        }

        #[cfg(feature = "metrics")]
        let start = metrics::now();

        // execute and mine
        let receipts = ExternalReceipts::from(receipts);
        if let Err(e) = executor.reexecute_external(&block, &receipts).await {
            let message = GlobalState::shutdown_from(TASK_NAME, "failed to reexecute external block");
            return log_and_err!(reason = e, message);
        };

        // statistics
        #[cfg(feature = "metrics")]
        {
            let duration = start.elapsed();
            let tps = calculate_tps(duration, block.transactions.len());

            tracing::info!(
                tps,
                duraton = %duration.to_string_ext(),
                block_number = ?block.number(),
                receipts = receipts.len(),
                "reexecuted external block",
            );
        }

        if let Err(e) = miner.mine_external_mixed_and_commit().await {
            let message = GlobalState::shutdown_from(TASK_NAME, "failed to mine external block");
            return log_and_err!(reason = e, message);
        };

        #[cfg(feature = "metrics")]
        {
            metrics::inc_n_importer_online_transactions_total(receipts.len() as u64);
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
        if GlobalState::warn_if_shutdown(TASK_NAME) {
            return Ok(());
        }

        // if we have a subscription, try to read from subscription.
        // in case of failure, re-subscribe because current subscription may have been closed in the server.
        if let Some(sub) = &mut sub_new_heads {
            tracing::info!("{} awaiting block number from newHeads subscription", TASK_NAME);
            let resubscribe_ws = match timeout(TIMEOUT_NEW_HEADS, sub.next()).await {
                Ok(Some(Ok(block))) => {
                    tracing::info!(number = %block.number(), "{} received newHeads event", TASK_NAME);
                    set_external_rpc_current_block(block.number());
                    continue;
                }
                Ok(None) => {
                    tracing::error!("{} newHeads subscription closed by the other side", TASK_NAME);
                    true
                }
                Ok(Some(Err(e))) => {
                    tracing::error!(reason = ?e, "{} failed to read newHeads subscription event", TASK_NAME);
                    true
                }
                Err(_) => {
                    tracing::error!("{} timed-out waiting for newHeads subscription event", TASK_NAME);
                    true
                }
            };

            // resubscribe if necessary.
            // only update the existing subscription if succedeed, otherwise we will try again in the next iteration.
            if chain.supports_ws() && resubscribe_ws {
                tracing::info!("{} resubscribing to newHeads event", TASK_NAME);
                match chain.subscribe_new_heads().await {
                    Ok(sub) => {
                        tracing::info!("{} resubscribed to newHeads event", TASK_NAME);
                        sub_new_heads = Some(sub);
                    }
                    Err(e) => {
                        tracing::error!(reason = ?e, "{} failed to resubscribe to newHeads event", TASK_NAME);
                    }
                }
            }
        }

        // fallback to polling
        tracing::warn!("{} falling back to http polling because subscription failed or it is not enabled", TASK_NAME);
        match chain.fetch_block_number().await {
            Ok(number) => {
                tracing::info!(
                    %number,
                    sync_interval = %sync_interval.to_string_ext(),
                    "fetched current block number via http. awaiting sync interval to retrieve again."
                );
                set_external_rpc_current_block(number);
                sleep(sync_interval).await;
            }
            Err(e) => {
                tracing::error!(reason = ?e, "failed to retrieve block number. retrying now.");
            }
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

    loop {
        if GlobalState::warn_if_shutdown(TASK_NAME) {
            return Ok(());
        }

        // if we are ahead of current block number, await until we are behind again
        let external_rpc_current_block = EXTERNAL_RPC_CURRENT_BLOCK.load(Ordering::Relaxed);
        if importer_block_number.as_u64() > external_rpc_current_block {
            yield_now().await;
            continue;
        }

        // we are behind current, so we will fetch multiple blocks in parallel to catch up
        let blocks_behind = external_rpc_current_block.saturating_sub(importer_block_number.as_u64()) + 1;
        let mut blocks_to_fetch = min(blocks_behind, 1_000); // avoid spawning millions of tasks (not parallelism), at least until we know it is safe
        tracing::info!(%blocks_behind, blocks_to_fetch, "catching up with blocks");

        let mut tasks = Vec::with_capacity(blocks_to_fetch as usize);
        while blocks_to_fetch > 0 {
            blocks_to_fetch -= 1;
            tasks.push(fetch_block_and_receipts(Arc::clone(&chain), importer_block_number));
            importer_block_number = importer_block_number.next();
        }

        // keep fetching in order
        let mut tasks = futures::stream::iter(tasks).buffered(PARALLEL_BLOCKS);
        while let Some((block, receipts)) = tasks.next().await {
            if backlog_tx.send((block, receipts)).is_err() {
                warn_task_rx_closed(TASK_NAME);
                return Ok(());
            }
        }
    }
}

#[tracing::instrument(skip_all)]
async fn fetch_block_and_receipts(chain: Arc<BlockchainClient>, number: BlockNumber) -> (ExternalBlock, Vec<ExternalReceipt>) {
    // fetch block
    let block = fetch_block(Arc::clone(&chain), number).await;

    // wait some time until receipts are available
    let _ = sleep(INTERVAL_FETCH_RECEIPTS).await;

    // fetch receipts in parallel
    let mut receipts_tasks = Vec::with_capacity(block.transactions.len());
    for hash in block.transactions.iter().map(|tx| tx.hash()) {
        receipts_tasks.push(fetch_receipt(Arc::clone(&chain), number, hash));
    }
    let receipts = futures::stream::iter(receipts_tasks).buffer_unordered(PARALLEL_RECEIPTS).collect().await;

    (block, receipts)
}

#[tracing::instrument(skip_all)]
async fn fetch_block(chain: Arc<BlockchainClient>, number: BlockNumber) -> ExternalBlock {
    let mut backoff = 10;
    loop {
        tracing::info!(%number, "fetching block");
        let block = match chain.fetch_block(number).await {
            Ok(json) => json,
            Err(e) => {
                backoff *= 2;
                backoff = min(backoff, 1000); // no more than 1000ms of backoff
                tracing::warn!(reason = ?e, %number, %backoff, "failed to retrieve block. retrying with backoff.");
                sleep(Duration::from_millis(backoff)).await;
                continue;
            }
        };

        if block.is_null() {
            #[cfg(not(feature = "perf"))]
            {
                backoff *= 2;
                backoff = min(backoff, 1000); // no more than 1000ms of backoff
                tracing::warn!(%number, "block not available yet because block is not mined. retrying with backoff.");
                sleep(Duration::from_millis(backoff)).await;
                continue;
            }

            #[cfg(feature = "perf")]
            std::process::exit(0);
        }

        return ExternalBlock::deserialize(&block).expect("cannot fail to deserialize external block");
    }
}

#[tracing::instrument(skip_all)]
async fn fetch_receipt(chain: Arc<BlockchainClient>, number: BlockNumber, hash: Hash) -> ExternalReceipt {
    loop {
        tracing::info!(%number, %hash, "fetching receipt");

        match chain.fetch_receipt(hash).await {
            Ok(Some(receipt)) => return receipt,
            Ok(None) => {
                tracing::warn!(%number, %hash, "receipt not available yet because block is not mined. retrying now.");
                continue;
            }
            Err(e) => {
                tracing::error!(reason = ?e, %number, %hash, "failed to fetch receipt. retrying now.");
            }
        }
    }
}
