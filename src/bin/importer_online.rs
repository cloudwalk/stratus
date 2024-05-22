use std::cmp::min;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use futures::try_join;
use futures::StreamExt;
use serde::Deserialize;
use stratus::config::ImporterOnlineConfig;
use stratus::eth::primitives::BlockNumber;
use stratus::eth::primitives::ExternalBlock;
use stratus::eth::primitives::ExternalReceipt;
use stratus::eth::primitives::ExternalReceipts;
use stratus::eth::primitives::Hash;
use stratus::eth::storage::StratusStorage;
use stratus::eth::BlockMiner;
use stratus::eth::Executor;
use stratus::ext::DisplayExt;
#[cfg(feature = "metrics")]
use stratus::infra::metrics;
use stratus::infra::tracing::warn_task_rx_closed;
use stratus::infra::tracing::warn_task_tx_closed;
use stratus::infra::BlockchainClient;
use stratus::GlobalServices;
use stratus::GlobalState;
use tokio::sync::mpsc;
use tokio::task::yield_now;
use tokio::time::sleep;
use tokio::time::timeout;

// -----------------------------------------------------------------------------
// Globals
// -----------------------------------------------------------------------------

/// Current block number used by the number fetcher and block fetcher.
///
/// It is a global to avoid unnecessary synchronization using a channel.
static RPC_CURRENT_BLOCK: AtomicU64 = AtomicU64::new(0);

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------
/// Number of blocks that are downloaded in parallel.
const PARALLEL_BLOCKS: usize = 3;

/// Number of receipts that are downloaded in parallel.
const PARALLEL_RECEIPTS: usize = 100;

/// Timeout for new newHeads event before fallback to polling.
const TIMEOUT_NEW_HEADS: Duration = Duration::from_millis(2000);

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
    let relayer = config.relayer.init(Arc::clone(&storage)).await?;
    let miner = config.miner.init(Arc::clone(&storage), None).await?;
    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner), relayer, None).await; //XXX TODO implement the consensus here, in case of it being a follower, it should not even enter here
    let chain = Arc::new(BlockchainClient::new_http_ws(&config.base.external_rpc, config.base.external_rpc_ws.as_deref()).await?);

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
    // start from last imported block
    let mut number = storage.read_mined_block_number().await?;
    if number != BlockNumber::from(0) {
        number = number.next();
    }

    let (backlog_tx, backlog_rx) = mpsc::unbounded_channel();

    // spawn block executor:
    // it executes and mines blocks and expects to receive them via channel in the correct order.
    let task_executor = tokio::spawn(start_block_executor(executor, miner, backlog_rx));

    // spawn block number:
    // it keeps track of the blockchain current block number.
    let number_fetcher_chain = Arc::clone(&chain);
    let task_number_fetcher = tokio::spawn(start_number_fetcher(number_fetcher_chain, sync_interval));

    // spawn block fetcher:
    // it fetches blocks and receipts in parallel and sends them to the executor in the correct order.
    // it uses the number fetcher current block to determine if should keep downloading more blocks or not.
    let block_fetcher_chain = Arc::clone(&chain);
    let task_block_fetcher = tokio::spawn(start_block_fetcher(block_fetcher_chain, backlog_tx, number));

    // await all tasks
    try_join!(task_executor, task_block_fetcher, task_number_fetcher)?;
    Ok(())
}

// -----------------------------------------------------------------------------
// Executor
// -----------------------------------------------------------------------------

// Executes external blocks and persist them to storage.
async fn start_block_executor(executor: Arc<Executor>, miner: Arc<BlockMiner>, mut backlog_rx: mpsc::UnboundedReceiver<(ExternalBlock, Vec<ExternalReceipt>)>) {
    const TASK_NAME: &str = "block-executor";

    while let Some((block, receipts)) = backlog_rx.recv().await {
        if GlobalState::warn_if_shutdown(TASK_NAME) {
            return;
        }

        #[cfg(feature = "metrics")]
        let start = metrics::now();

        // execute and mine
        let receipts = ExternalReceipts::from(receipts);
        if executor.reexecute_external(&block, &receipts).await.is_err() {
            GlobalState::shutdown_from(TASK_NAME, "failed to re-execute block");
            return;
        };
        if miner.mine_external_mixed_and_commit().await.is_err() {
            GlobalState::shutdown_from(TASK_NAME, "failed to mine external block");
            return;
        };

        #[cfg(feature = "metrics")]
        {
            metrics::inc_n_importer_online_transactions_total(receipts.len() as u64);
            metrics::inc_import_online_mined_block(start.elapsed());
        }
    }

    warn_task_tx_closed(TASK_NAME);
}

// -----------------------------------------------------------------------------
// Number fetcher
// -----------------------------------------------------------------------------

/// Retrieves the blockchain current block number.
async fn start_number_fetcher(chain: Arc<BlockchainClient>, sync_interval: Duration) {
    const TASK_NAME: &str = "external-number-fetcher";

    // subscribe to newHeads event if WS is enabled
    let mut sub_new_heads = match chain.is_ws_enabled() {
        true => {
            tracing::info!("subscribing {} to newHeads event", TASK_NAME);
            match chain.subscribe_new_heads().await {
                Ok(sub) => Some(sub),
                Err(_) => {
                    GlobalState::shutdown_from(TASK_NAME, "cannot subscribe to newHeads event");
                    return;
                }
            }
        }
        false => {
            tracing::warn!("{} blockchain client does not have websocket enabled", TASK_NAME);
            None
        }
    };

    loop {
        if GlobalState::warn_if_shutdown(TASK_NAME) {
            return;
        }
        tracing::info!("fetching current block number");

        // if we have a subscription, try to read from subscription.
        // in case of failure, re-subscribe because current subscription may have been dropped in the server.
        if let Some(sub) = &mut sub_new_heads {
            let resubscribe = match timeout(TIMEOUT_NEW_HEADS, sub.next()).await {
                Ok(Some(Ok(block))) => {
                    tracing::info!(number = %block.number(), "newHeads event received");
                    RPC_CURRENT_BLOCK.store(block.number().as_u64(), Ordering::SeqCst);
                    continue;
                }
                Ok(None) => {
                    tracing::error!("newHeads subscription closed by the other side");
                    true
                }
                Ok(Some(Err(e))) => {
                    tracing::error!(reason = ?e, "failed to read newHeads subscription event");
                    true
                }
                Err(_) => {
                    tracing::error!("timeout waiting for newHeads subscription event");
                    true
                }
            };

            // resubscribe if necessary
            // only update the existing subscription if succedeed, otherwise we will try again in the iteration of the loop
            if chain.is_ws_enabled() && resubscribe {
                tracing::info!("resubscribing to newHeads event");
                match chain.subscribe_new_heads().await {
                    Ok(sub) => sub_new_heads = Some(sub),
                    Err(e) => {
                        tracing::error!(reason = ?e, "failed to resubscribe number-fetcher to newHeads event");
                    }
                }
            }
        }

        // fallback to polling
        tracing::warn!("number-fetcher falling back to http polling because subscription failed or it not enabled");
        match chain.get_current_block_number().await {
            Ok(number) => {
                tracing::info!(
                    %number,
                    sync_interval = %sync_interval.to_string_ext(),
                    "fetched current block number via http. awaiting sync interval to retrieve again."
                );
                RPC_CURRENT_BLOCK.store(number.as_u64(), Ordering::SeqCst);
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
async fn start_block_fetcher(chain: Arc<BlockchainClient>, backlog_tx: mpsc::UnboundedSender<(ExternalBlock, Vec<ExternalReceipt>)>, mut number: BlockNumber) {
    const TASK_NAME: &str = "external-block-fetcher";

    loop {
        if GlobalState::warn_if_shutdown(TASK_NAME) {
            return;
        }

        // if we are ahead of current block number, await until we are behind again
        let rpc_current_number = RPC_CURRENT_BLOCK.load(Ordering::SeqCst);
        if number.as_u64() > rpc_current_number {
            yield_now().await;
            continue;
        }

        // we are behind current, so we will fetch multiple blocks in parallel to catch up
        let mut block_diff = rpc_current_number.saturating_sub(number.as_u64());
        block_diff = min(block_diff, 1_000); // avoid spawning millions of tasks (not parallelism), at least until we know it is safe

        let mut tasks = Vec::with_capacity(block_diff as usize);
        while block_diff > 0 {
            block_diff -= 1;
            tasks.push(fetch_block_and_receipts(Arc::clone(&chain), number));
            number = number.next();
        }

        // keep fetching in order
        let mut tasks = futures::stream::iter(tasks).buffered(PARALLEL_BLOCKS);
        while let Some((block, receipts)) = tasks.next().await {
            if backlog_tx.send((block, receipts)).is_err() {
                warn_task_rx_closed(TASK_NAME);
                return;
            }
        }
    }
}

#[tracing::instrument(skip_all)]
async fn fetch_block_and_receipts(chain: Arc<BlockchainClient>, number: BlockNumber) -> (ExternalBlock, Vec<ExternalReceipt>) {
    // fetch block
    let block = fetch_block(Arc::clone(&chain), number).await;

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
        let block = match chain.get_block_by_number(number).await {
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

        match chain.get_transaction_receipt(hash).await {
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
