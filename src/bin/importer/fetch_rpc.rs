use std::cmp::min;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use clap::Parser;
use futures::StreamExt;
use futures::TryStreamExt;
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use stratus::config::Config;
use stratus::config::StorageConfig;
use stratus::eth::primitives::BlockNumber;
use stratus::infra::init_tracing;
use stratus::infra::postgres::Postgres;
use stratus::infra::BlockchainClient;
use tokio::time::timeout;

/// Number of parallel downloads to execute.
const WORKERS: usize = 2;

/// Number of blocks each parallel download will process.
const WORKER_BLOCKS: usize = 10_000;

/// Timeout for network operations.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(2);

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    // init global
    let config = Config::parse();
    init_tracing();

    // init storage
    let pg = match config.storage {
        StorageConfig::Postgres { url } => Postgres::new(&url).await?.connection_pool,
        StorageConfig::InMemory => panic!("unsupported storage: inmemory"),
    };

    // init blockchain
    let rpc = config.external_rpc.expect("--external-rpc is required"); // todo: clap commands
    let chain = Arc::new(BlockchainClient::new(&rpc)?);

    // generate tasks
    let mut task_start = BlockNumber::ZERO;
    let current_block = chain.get_current_block_number().await.unwrap();
    tracing::info!(%current_block, "current block number");

    let mut tasks = Vec::new();
    while task_start < current_block {
        let task_end = min(task_start + (WORKER_BLOCKS - 1), current_block);
        tasks.push(Task::new(pg.clone(), Arc::clone(&chain), task_start, task_end)?.execute());
        task_start += WORKER_BLOCKS;
    }

    // execute tasks in threadpool
    tracing::info!(tasks = %tasks.len(), "submitting tasks");
    let result = futures::stream::iter(tasks).buffer_unordered(WORKERS).try_collect::<Vec<()>>().await;
    match result {
        Ok(_) => {
            tracing::info!("tasks finished");
            Ok(())
        }
        Err(e) => {
            tracing::error!(reason = ?e, "tasks failed");
            Err(e)
        }
    }
}

struct Task {
    pg: PgPool,
    chain: Arc<BlockchainClient>,
    start: BlockNumber,
    end_inclusive: BlockNumber,
}

impl Task {
    pub fn new(pg: PgPool, chain: Arc<BlockchainClient>, start: BlockNumber, end_inclusive: BlockNumber) -> anyhow::Result<Self> {
        Ok(Self {
            pg,
            chain,
            start,
            end_inclusive,
        })
    }

    pub async fn execute(self) -> anyhow::Result<()> {
        // calculate current block
        let mut current = match db_max_downloaded_block(&self.pg, self.start, self.end_inclusive).await? {
            Some(max_block) => max_block.next(),
            None => self.start,
        };
        tracing::info!(start = %self.start, current = %current, end = %self.end_inclusive, "starting");

        // download blocks
        while current <= self.end_inclusive {
            tracing::info!(current = %current, end = %self.end_inclusive, "downloading");

            loop {
                // retrieve block
                let Ok(Ok(block)) = timeout(NETWORK_TIMEOUT, self.chain.get_block_by_number(current)).await else {
                    tracing::warn!("retrying");
                    continue;
                };
                if let Err(e) = db_insert_block(&self.pg, current, block).await {
                    tracing::warn!(reason = ?e, "retrying because failed to save block");
                    continue;
                }

                current = current.next();
                break;
            }
        }
        Ok(())
    }
}

async fn db_max_downloaded_block(db: &PgPool, start: BlockNumber, end: BlockNumber) -> anyhow::Result<Option<BlockNumber>> {
    tracing::debug!(%start, %end, "counting collected blocks");

    let result = sqlx::query_file_scalar!("src/bin/importer/sql/select_max_downloaded_block_in_range.sql", start.as_i64(), end.as_i64())
        .fetch_one(db)
        .await;
    let block_number: i64 = match result {
        Ok(Some(max)) => max,
        Ok(None) => return Ok(None),
        Err(e) => {
            tracing::error!(reason = ?e, "failed to retrieve max block number");
            return Err(e).context("failed to retrieve max block number");
        }
    };

    Ok(Some(BlockNumber::from(block_number)))
}

async fn db_insert_block(db: &PgPool, number: BlockNumber, block: JsonValue) -> anyhow::Result<()> {
    tracing::debug!(payload = ?block, "saving block");

    let result = sqlx::query_file!("src/bin/importer/sql/insert_block.sql", number.as_i64(), block)
        .execute(db)
        .await;
    match result {
        Ok(_) => Ok(()),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            tracing::warn!(reason = ?e, "block unique violation, skipping");
            Ok(())
        }
        Err(e) => {
            tracing::error!(reason = ?e, "failed to insert block");
            Err(e).context("failed to insert block")
        }
    }
}
