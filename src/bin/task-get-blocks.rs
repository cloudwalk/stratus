use std::cmp::min;
use std::env;
use std::fs::OpenOptions;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;
use stratus::eth::primitives::BlockNumber;
use stratus::ext::not;
use stratus::infra::init_tracing;
use stratus::infra::BlockchainClient;
use tokio::runtime::Handle;

/// Number of workers to spawn.
const WORKERS: usize = 5;

/// Number of blocks each block will process.
const WORKER_BLOCKS: usize = 10_000;

#[tokio::main]
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
        tasks.push(Task::new(Arc::clone(&chain), task_start, task_end)?);
        task_start += WORKER_BLOCKS;
    }

    // execute tasks in threadpool
    tracing::info!(tasks = %tasks.len(), "submitting tasks");
    let (tx_error, rx_error) = mpsc::channel();
    let tp = threadpool::Builder::new().thread_name("block-poller".to_string()).num_threads(WORKERS).build();
    for task in tasks {
        let tx_error_for_task = tx_error.clone();
        let tokio = Handle::current();
        tp.execute(move || {
            let _tokio_guard = tokio.enter();
            if let Err(e) = task.execute() {
                let _ = tx_error_for_task.send(e);
            }
        });
    }

    // await threadpool to finish or error to be produced
    loop {
        if let Ok(e) = rx_error.recv_timeout(Duration::from_secs(1)) {
            return Err(e);
        }

        let running = tp.active_count() > 0 || tp.queued_count() > 0;
        if not(running) {
            tracing::info!("finished all tasks");
            break;
        }
    }

    Ok(())
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

    pub fn execute(self) -> anyhow::Result<()> {
        // calculate current block
        let mut current = self.start + self.downloaded_blocks().unwrap_or(0);
        tracing::info!(start = %self.start, current = %current, end = %self.end, "starting");

        // prepare output
        let mut file = match OpenOptions::new().create(true).append(true).open(self.filename.clone()) {
            Ok(file) => file,
            Err(e) => {
                tracing::error!(%e, filename = %self.filename.clone(), "failed to open file for appending");
                return Err(e).context(format!("failed to open file \"{}\" for appending", self.filename.clone()));
            }
        };

        // download
        while current < self.end {
            tracing::debug!(start = %self.start, current = %current, end = %self.end, "downloading");
            match Handle::current().block_on(self.chain.get_block_by_number(current)) {
                Ok(block) => {
                    let block_json = serde_json::to_string(&block).unwrap();
                    writeln!(file, "{}", block_json)?;
                }
                Err(_) => {
                    // RETRY HOW MANY TIMES?
                }
            };
            current = current.next();
        }
        Ok(())
    }

    fn downloaded_blocks(&self) -> anyhow::Result<usize> {
        let file = match OpenOptions::new().read(true).open(self.filename.clone()) {
            Ok(file) => file,
            Err(e) => {
                tracing::error!(%e, filename = %self.filename.clone(), "failed to open file for reading");
                return Err(e).context(format!("failed to open file \"{}\" for reading", self.filename.clone()));
            }
        };

        Ok(BufReader::new(file).lines().count())
    }
}
