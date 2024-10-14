use std::cmp::min;
use std::iter;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;
use futures::stream;
use futures::StreamExt;
use itertools::Itertools;
use serde::Deserialize;
use stratus::config::RpcDownloaderConfig;
use stratus::eth::primitives::Address;
use stratus::eth::primitives::BlockNumber;
use stratus::eth::primitives::Hash;
use stratus::eth::storage::ExternalRpcStorage;
use stratus::ext::not;
use stratus::infra::BlockchainClient;
use stratus::utils::DropTimer;
use stratus::GlobalServices;
use stratus::GlobalState;
#[cfg(all(not(target_env = "msvc"), any(feature = "jemalloc", feature = "jeprof")))]
use tikv_jemallocator::Jemalloc;

#[cfg(all(not(target_env = "msvc"), any(feature = "jemalloc", feature = "jeprof")))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

/// Number of blocks each parallel download will process.
const BLOCKS_BY_TASK: usize = 1_000;

static BLOCKS_DOWNLOADED: AtomicU32 = AtomicU32::new(0);

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<RpcDownloaderConfig>::init();
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: RpcDownloaderConfig) -> anyhow::Result<()> {
    let _timer = DropTimer::start("rpc-downloader");

    if not(config.external_rpc.contains("app=") || config.external_rpc.contains("/app/")) {
        tracing::warn!(url = config.external_rpc, "url isn't identified with '?app=NAME' query parameter");
    }

    let rpc_storage = config.rpc_storage.init().await?;
    let chain = Arc::new(BlockchainClient::new_http(&config.external_rpc, config.external_rpc_timeout).await?);

    let block_end = match config.block_end {
        Some(end) => BlockNumber::from(end),
        None => chain.fetch_block_number().await?,
    };

    // download balances and blocks
    download_balances(Arc::clone(&rpc_storage), &chain, config.initial_accounts).await?;
    download_blocks(rpc_storage, chain, config.paralellism, block_end).await?;

    Ok(())
}

async fn download_balances(rpc_storage: Arc<dyn ExternalRpcStorage>, chain: &BlockchainClient, accounts: Vec<Address>) -> anyhow::Result<()> {
    let _timer = DropTimer::start("rpc-downloader::download_balances");

    if accounts.is_empty() {
        tracing::warn!("no initial accounts to retrieve balance");
        return Ok(());
    } else {
        tracing::info!(?accounts, "retrieving initial balances");
    }

    // retrieve downloaded balances
    let downloaded_accounts = rpc_storage.read_initial_accounts().await?;
    let downloaded_accounts_addresses = downloaded_accounts.iter().map(|a| &a.address).collect_vec();

    // keep only accounts that must be downloaded
    let address_to_download = accounts
        .into_iter()
        .filter(|address| not(downloaded_accounts_addresses.contains(&address)))
        .collect_vec();

    // download missing balances
    for address in address_to_download {
        let balance = chain.fetch_balance(&address, Some(BlockNumber::ZERO)).await?;
        rpc_storage.save_initial_account(address, balance).await?;
    }

    Ok(())
}

async fn download_blocks(rpc_storage: Arc<dyn ExternalRpcStorage>, chain: Arc<BlockchainClient>, paralellism: usize, end: BlockNumber) -> anyhow::Result<()> {
    const TASK_NAME: &str = "rpc-downloader::download_blocks";
    let _timer = DropTimer::start(TASK_NAME);

    // prepare download block tasks
    let mut start = BlockNumber::ZERO;

    tracing::info!(blocks_by_taks = %BLOCKS_BY_TASK, %start, %end, "preparing block downloads");

    let mut tasks = Vec::new();
    while start <= end {
        let end = min(start + (BLOCKS_BY_TASK - 1), end);
        tasks.push(download(Arc::clone(&rpc_storage), Arc::clone(&chain), start, end));
        start += BLOCKS_BY_TASK;
    }

    // execute download block tasks
    tracing::info!(tasks = %tasks.len(), %paralellism, "executing block downloads");

    thread::spawn(blocks_per_minute_reporter);

    let mut stream = stream::iter(tasks).buffered(paralellism);
    while let Some(result) = stream.next().await {
        if let Err(e) = result {
            tracing::error!(reason = ?e, "download task failed");
        }

        if GlobalState::is_shutdown_warn(TASK_NAME) {
            break;
        }
    }

    tracing::info!("download finished");
    Ok(())
}

async fn download(
    rpc_storage: Arc<dyn ExternalRpcStorage>,
    chain: Arc<BlockchainClient>,
    start: BlockNumber,
    end_inclusive: BlockNumber,
) -> anyhow::Result<()> {
    // calculate current block
    let mut current = match rpc_storage.read_max_block_number_in_range(start, end_inclusive).await? {
        Some(number) => number.next_block_number(),
        None => start,
    };
    tracing::info!(%start, current = %current, end = %end_inclusive, "starting download task (might skip)");

    // download blocks
    while current <= end_inclusive {
        loop {
            // retrieve block
            let block_json = match chain.fetch_block(current).await {
                Ok(json) => json,
                Err(e) => {
                    tracing::warn!(reason = ?e, "retrying block download");
                    continue;
                }
            };

            // extract transaction hashes
            let block: ImporterBlock = match ImporterBlock::deserialize(&block_json) {
                Ok(block) => block,
                Err(e) => {
                    tracing::error!(reason = ?e, block_number = %current, payload = ?block_json, "block does not match expected format");
                    return Err(e).context(format!("block does not match expected format for block {}", current));
                }
            };
            let hashes = block.transactions.into_iter().map(|tx| tx.hash).collect_vec();

            // retrieve receipts
            let mut receipts_json = Vec::with_capacity(hashes.len());
            for tx_hash in hashes {
                loop {
                    let receipt = match chain.fetch_receipt(tx_hash).await {
                        Ok(receipt) => receipt,
                        Err(e) => {
                            tracing::warn!(reason = ?e, "retrying receipt download");
                            continue;
                        }
                    };

                    match receipt {
                        Some(receipt) => {
                            receipts_json.push((tx_hash, receipt));
                            break;
                        }
                        None => {
                            tracing::error!(%tx_hash, payload = ?receipt, "receipt is null");
                            return Err(anyhow!(format!("transaction receipt is null for hash {}", tx_hash)));
                        }
                    }
                }
            }

            // save block and receipts
            if let Err(e) = rpc_storage.save_block_and_receipts(current, block_json, receipts_json).await {
                tracing::warn!(reason = ?e, "retrying because failed to save block");
                continue;
            }

            BLOCKS_DOWNLOADED.fetch_add(1, Ordering::Relaxed);

            current = current.next_block_number();
            break;
        }
    }

    Ok(())
}

fn blocks_per_minute_reporter() {
    let mut intervals_in_minutes = (1..10).into_iter().chain(iter::repeat(10));

    loop {
        let interval = intervals_in_minutes.next().expect("infinite iterator");
        let interval = Duration::from_secs(interval * 60);

        let block_before = BLOCKS_DOWNLOADED.load(Ordering::Relaxed);
        thread::sleep(interval);
        let block_after = BLOCKS_DOWNLOADED.load(Ordering::Relaxed);

        let block_diff = block_after - block_before;

        let blocks_per_second = block_diff as f64 / interval.as_secs() as f64;

        tracing::info!(
            blocks_per_second = format_args!("{blocks_per_second:.2}"),
            blocks_per_day = (blocks_per_second * 60.0 * 60.0 * 24.0) as u32,
            sample_interval = ?interval,
            "speed report",
        );
    }
}

// -----------------------------------------------------------------------------
// Blockchain RPC structs
// -----------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct ImporterBlock {
    transactions: Vec<ImporterTransaction>,
}

#[derive(serde::Deserialize)]
struct ImporterTransaction {
    hash: Hash,
}
