use std::cmp::min;
use std::env;
use std::fs::OpenOptions;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;
use futures::StreamExt;
use futures::TryStreamExt;
use stratus::eth::primitives::BlockNumber;
use stratus::infra::init_tracing;
use stratus::infra::BlockchainClient;
use tokio::time::timeout;

/// Number of parallel downloads to execute.
const WORKERS: usize = 5;

/// Number of blocks each parallel download will process.
const WORKER_BLOCKS: usize = 10_000;

/// Timeout for network operations.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(2);

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    // init services
    init_tracing();
    let Some(rpc_url) = env::args().nth(1) else {
        return Err(anyhow!("rpc url not provided as argument"));
    };
    let chain = Arc::new(BlockchainClient::new(&rpc_url)?);

    // generate tasks
    let mut task_start = BlockNumber::ZERO;
    let current_block = chain.get_current_block_number().await.unwrap();
    tracing::info!(%current_block, "current block number");

    let mut tasks = Vec::new();
    while task_start < current_block {
        let task_end = min(task_start + WORKER_BLOCKS, current_block);
        tasks.push(Task::new(Arc::clone(&chain), task_start, task_end)?.execute());
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
    chain: Arc<BlockchainClient>,
    start: BlockNumber,
    end: BlockNumber,
    filename: String,
}

impl Task {
    pub fn new(chain: Arc<BlockchainClient>, start: BlockNumber, end: BlockNumber) -> anyhow::Result<Self> {
        let filename = format!("data/{}.json", start);
        Ok(Self { chain, start, end, filename })
    }

    pub async fn execute(self) -> anyhow::Result<()> {
        // calculate current block
        let mut current = self.start + self.downloaded_blocks().unwrap_or(0);
        tracing::info!(start = %self.start, current = %current, end = %self.end, "starting");

        // prepare output file
        let mut file = match OpenOptions::new().create(true).append(true).open(self.filename.clone()) {
            Ok(file) => file,
            Err(e) => {
                tracing::error!(reason = ?e, filename = %self.filename.clone(), "failed to open file for appending");
                return Err(e).context(format!("failed to open file \"{}\" for appending", self.filename.clone()));
            }
        };

        // download blocks
        while current < self.end {
            tracing::debug!(start = %self.start, current = %current, end = %self.end, "downloading");

            // keep trying to download until success
            loop {
                match timeout(NETWORK_TIMEOUT, self.chain.get_block_by_number(current)).await {
                    // write block to file
                    // abort process in case of failure because we don't want to lose data
                    Ok(Ok(block)) => {
                        let block_json = match serde_json::to_string(&block) {
                            Ok(json) => json,
                            Err(e) => {
                                tracing::error!("failed to serialize block to json");
                                return Err(e).context("failed to serialize block to json");
                            }
                        };
                        if let Err(e) = writeln!(file, "{}", block_json) {
                            tracing::error!(filename = %self.filename.clone(), "failed to write block to file");
                            return Err(e).context("failed to write block to file");
                        };
                        break;
                    }
                    // just retry because it is timeout
                    Err(e) => {
                        tracing::warn!(reason = ?e, "retrying block download");
                    }
                    // just retry because it is network error
                    Ok(Err(e)) => {
                        tracing::warn!(reason = ?e, "retrying block download");
                    }
                }
            }

            current = current.next();
        }
        Ok(())
    }

    fn downloaded_blocks(&self) -> anyhow::Result<usize> {
        let file = match OpenOptions::new().read(true).open(self.filename.clone()) {
            Ok(file) => file,
            Err(e) => {
                tracing::error!(reason = ?e, filename = %self.filename.clone(), "failed to open file for reading");
                return Err(e).context(format!("failed to open file \"{}\" for reading", self.filename.clone()));
            }
        };
        Ok(BufReader::new(file).lines().count())
    }
}
