mod helpers;

use std::cmp::min;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;
use futures::StreamExt;
use futures::TryStreamExt;
use helpers::*;
use itertools::Itertools;
use stratus::config::RpcDownloaderConfig;
use stratus::eth::primitives::Address;
use stratus::eth::primitives::BlockNumber;
use stratus::eth::primitives::Hash;
use stratus::ext::not;
use stratus::infra::postgres::Postgres;
use stratus::infra::BlockchainClient;
use stratus::init_global_services;
use stratus::log_and_err;

/// Number of blocks each parallel download will process.
const BLOCKS_BY_TASK: usize = 1_000;

/// Timeout for network operations.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(2);

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    // init services
    let config: RpcDownloaderConfig = init_global_services();
    let pg = Arc::new(Postgres::new(&config.postgres_url).await?);
    let chain = Arc::new(BlockchainClient::new(&config.external_rpc, NETWORK_TIMEOUT)?);

    // download balances and blocks
    download_balances(&pg, &chain, config.initial_accounts).await?;
    download_blocks(pg, chain, config.paralellism).await?;
    Ok(())
}

async fn download_balances(pg: &Postgres, chain: &BlockchainClient, accounts: Vec<Address>) -> anyhow::Result<()> {
    if accounts.is_empty() {
        tracing::warn!("no initial accounts to retrieve balance");
        return Ok(());
    } else {
        tracing::info!(?accounts, "retrieving initial balances");
    }

    // retrieve downloaded balances
    let downloaded_balances = pg_retrieve_external_balances(pg).await?;
    let downloaded_balances_addresses = downloaded_balances.iter().map(|balance| &balance.address).collect_vec();

    // keep only accounts that must be downloaded
    let address_to_download = accounts
        .into_iter()
        .filter(|address| not(downloaded_balances_addresses.contains(&address)))
        .collect_vec();

    // download missing balances
    for address in address_to_download {
        let balance = chain.get_balance(&address, Some(BlockNumber::ZERO)).await?;
        pg_insert_external_balance(pg, address, balance).await?;
    }

    Ok(())
}

async fn download_blocks(pg: Arc<Postgres>, chain: Arc<BlockchainClient>, paralellism: usize) -> anyhow::Result<()> {
    // prepare download block tasks
    let mut start = BlockNumber::ZERO;
    let end = chain.get_current_block_number().await?;
    tracing::info!(blocks_by_taks = %BLOCKS_BY_TASK, %start, %end, "preparing block downloads");

    let mut tasks = Vec::new();
    while start <= end {
        let end = min(start + (BLOCKS_BY_TASK - 1), end);
        tasks.push(download(Arc::clone(&pg), Arc::clone(&chain), start, end));
        start += BLOCKS_BY_TASK;
    }

    // execute download block tasks
    tracing::info!(tasks = %tasks.len(), %paralellism, "executing block downloads");
    let result = futures::stream::iter(tasks).buffer_unordered(paralellism).try_collect::<Vec<()>>().await;
    match result {
        Ok(_) => {
            tracing::info!("tasks finished");
            Ok(())
        }
        Err(e) => {
            log_and_err!(reason = e, "tasks failed")
        }
    }
}

async fn download(pg: Arc<Postgres>, chain: Arc<BlockchainClient>, start: BlockNumber, end_inclusive: BlockNumber) -> anyhow::Result<()> {
    // calculate current block
    let mut current = match pg_retrieve_max_external_block(&pg, start, end_inclusive).await? {
        Some(number) => number.next(),
        None => start,
    };
    tracing::info!(%start, current = %current, end = %end_inclusive, "starting");

    // download blocks
    while current <= end_inclusive {
        tracing::info!(number = %current, "downloading");

        loop {
            // retrieve block
            let block_json = match chain.get_block_by_number(current).await {
                Ok(json) => json,
                Err(e) => {
                    tracing::warn!(reason = ?e, "retrying block download");
                    continue;
                }
            };

            // extract transaction hashes
            let block: ImporterBlock = match serde_json::from_value(block_json.clone()) {
                Ok(block) => block,
                Err(e) => {
                    tracing::error!(reason = ?e, number = %current, payload = ?block_json, "block does not match expected format");
                    return Err(e).context(format!("block does not match expected format for block {}", current));
                }
            };
            let hashes = block.transactions.into_iter().map(|tx| tx.hash).collect_vec();

            // retrieve receipts
            let mut receipts_json = Vec::with_capacity(hashes.len());
            for hash in hashes {
                loop {
                    let receipt_json = match chain.get_transaction_receipt(&hash).await {
                        Ok(json) => json,
                        Err(e) => {
                            tracing::warn!(reason = ?e, "retrying receipt download");
                            continue;
                        }
                    };
                    if receipt_json.is_null() {
                        tracing::error!(%hash, payload = ?receipt_json, "receipt is null");
                        return Err(anyhow!(format!("transaction receipt is null for hash {}", hash)));
                    }

                    receipts_json.push((hash, receipt_json));
                    break;
                }
            }

            // save block and receipts
            if let Err(e) = pg_insert_external_block_and_receipts(&pg, current, block_json, receipts_json).await {
                tracing::warn!(reason = ?e, "retrying because failed to save block");
                continue;
            }

            current = current.next();
            break;
        }
    }
    Ok(())
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
