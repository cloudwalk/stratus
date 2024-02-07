use std::cmp::min;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;
use futures::Future;
use futures::StreamExt;
use futures::TryStreamExt;
use itertools::Itertools;
use serde_json::Value as JsonValue;
use stratus::config::ImporterDownloadConfig;
use stratus::eth::primitives::BlockNumber;
use stratus::eth::primitives::Hash;
use stratus::infra::postgres::Postgres;
use stratus::infra::BlockchainClient;
use stratus::init_global_services;
use stratus::log_and_err;

/// Number of parallel downloads to execute.
const PARALELLISM: usize = 20;

/// Number of blocks each parallel download will process.
const BLOCKS_BY_TASK: usize = 1_000;

/// Timeout for network operations.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(2);

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    // init services
    let config: ImporterDownloadConfig = init_global_services();
    let pg = Arc::new(Postgres::new(&config.postgres_url).await?);
    let chain = Arc::new(BlockchainClient::new(&config.external_rpc, NETWORK_TIMEOUT)?);

    // prepare and execute tasks
    let tasks = prepare_tasks(pg, chain).await?;
    execute_tasks(tasks).await
}

async fn prepare_tasks(pg: Arc<Postgres>, chain: Arc<BlockchainClient>) -> anyhow::Result<Vec<impl Future<Output = Result<(), anyhow::Error>>>> {
    let mut start = BlockNumber::ZERO;
    let end = chain.get_current_block_number().await?;

    tracing::info!(blocks_by_taks = %BLOCKS_BY_TASK, %start, %end, "preparing tasks");
    let mut tasks = Vec::new();
    while start <= end {
        let end = min(start + (BLOCKS_BY_TASK - 1), end);
        tasks.push(download(Arc::clone(&pg), Arc::clone(&chain), start, end));
        start += BLOCKS_BY_TASK;
    }
    Ok(tasks)
}

async fn execute_tasks(tasks: Vec<impl Future<Output = Result<(), anyhow::Error>>>) -> anyhow::Result<()> {
    tracing::info!(tasks = %tasks.len(), paralellism = %PARALELLISM, "executing tasks");
    let result = futures::stream::iter(tasks).buffer_unordered(PARALELLISM).try_collect::<Vec<()>>().await;
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
    let mut current = match find_max_downloaded_block(&pg, start, end_inclusive).await? {
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
            if let Err(e) = insert_block_and_receipts(&pg, current, block_json, receipts_json).await {
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
// Postgres
// -----------------------------------------------------------------------------

async fn find_max_downloaded_block(pg: &Postgres, start: BlockNumber, end: BlockNumber) -> anyhow::Result<Option<BlockNumber>> {
    tracing::debug!(%start, %end, "finding max downloaded block");

    let result = sqlx::query_file_scalar!("src/bin/importer/sql/select_max_downloaded_block_in_range.sql", start.as_i64(), end.as_i64())
        .fetch_one(&pg.connection_pool)
        .await;
    let block_number: i64 = match result {
        Ok(Some(max)) => max,
        Ok(None) => return Ok(None),
        Err(e) => return log_and_err!(reason = e, "failed to find max block number"),
    };

    Ok(Some(block_number.into()))
}

async fn insert_block_and_receipts(pg: &Postgres, number: BlockNumber, block: JsonValue, receipts: Vec<(Hash, JsonValue)>) -> anyhow::Result<()> {
    tracing::debug!(?block, ?receipts, "saving block and receipts");

    let mut tx = pg.start_transaction().await?;

    // insert block
    let result = sqlx::query_file!("src/bin/importer/sql/insert_block.sql", number.as_i64(), block)
        .execute(&mut *tx)
        .await;

    match result {
        Ok(_) => {}
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            tracing::warn!(reason = ?e, "block unique violation, skipping");
        }
        Err(e) => {
            return log_and_err!(reason = e, "failed to insert block");
        }
    }

    // insert receipts
    for (hash, receipt) in receipts {
        let result = sqlx::query_file!("src/bin/importer/sql/insert_receipt.sql", hash.as_ref(), number.as_i64(), receipt)
            .execute(&mut *tx)
            .await;

        match result {
            Ok(_) => {}
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                tracing::warn!(reason = ?e, "receipt unique violation, skipping");
            }
            Err(e) => {
                return log_and_err!(reason = e, "failed to insert receipt");
            }
        }
    }

    pg.commit_transaction(tx).await?;

    Ok(())
}

// -----------------------------------------------------------------------------
// Minimum RPC structs
// -----------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct ImporterBlock {
    transactions: Vec<ImporterTransaction>,
}

#[derive(serde::Deserialize)]
struct ImporterTransaction {
    hash: Hash,
}
