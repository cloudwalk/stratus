//! Importer-Offline binary.
//!
//! It loads blocks (and receipts) from an external RPC server, or from a PostgreSQL DB
//! that was prepared with the `rpc-downloader` binary.
//!
//! This importer will check on startup what is the `block_end` value at the external
//! storage, and will not update while running, in contrast with that, the
//! Importer-Online (other binary) will stay up to date with the newer blocks that
//! arrive.

use std::cmp::min;
use std::sync::Arc;

use anyhow::anyhow;
use futures::StreamExt;
use stratus::config::ImporterOfflineConfig;
use stratus::eth::executor::Executor;
use stratus::eth::miner::Miner;
use stratus::eth::miner::MinerMode;
use stratus::eth::primitives::BlockNumber;
use stratus::eth::primitives::ExternalReceipts;
use stratus::eth::primitives::ExternalTransaction;
use stratus::eth::storage::ExternalBlockWithReceipts;
use stratus::eth::storage::ExternalRpcStorage;
use stratus::ext::spawn_named;
use stratus::ext::spawn_thread;
use stratus::log_and_err;
use stratus::utils::calculate_tps_and_bpm;
use stratus::utils::DropTimer;
use stratus::GlobalServices;
use stratus::GlobalState;
#[cfg(all(not(target_env = "msvc"), any(feature = "jemalloc", feature = "jeprof")))]
use tikv_jemallocator::Jemalloc;
use tokio::sync::mpsc;
use tokio::time::Instant;

#[cfg(all(not(target_env = "msvc"), any(feature = "jemalloc", feature = "jeprof")))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;
/// Number of tasks in the backlog. Each task contains `--blocks-by-fetch` blocks and all receipts for them.
const BACKLOG_SIZE: usize = 10;

type BacklogTask = Vec<ExternalBlockWithReceipts>;

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<ImporterOfflineConfig>::init();
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: ImporterOfflineConfig) -> anyhow::Result<()> {
    let _timer = DropTimer::start("importer-offline");

    // init services
    let rpc_storage = config.rpc_storage.init().await?;
    let storage = config.storage.init()?;
    let miner = config.miner.init_with_mode(MinerMode::External, Arc::clone(&storage)).await?;
    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner));

    // init block range
    let block_start = match config.block_start {
        Some(start) => BlockNumber::from(start),
        None => storage.read_block_number_to_resume_import()?,
    };
    let block_end = match config.block_end {
        Some(end) => BlockNumber::from(end),
        None => block_number_to_stop(&rpc_storage).await?,
    };

    // init shared data between importer and external rpc storage loader
    let (backlog_tx, backlog_rx) = mpsc::channel::<BacklogTask>(BACKLOG_SIZE);

    // load genesis accounts
    let initial_accounts = rpc_storage.read_initial_accounts().await?;
    storage.save_accounts(initial_accounts.clone())?;

    let storage_loader = execute_external_rpc_storage_loader(rpc_storage, config.blocks_by_fetch, config.paralellism, block_start, block_end, backlog_tx);
    spawn_named("storage-loader", async move {
        if let Err(e) = storage_loader.await {
            tracing::error!(parent: None, reason = ?e, "'storage-loader' task failed");
        }
    });

    let block_importer = spawn_thread("block-importer", || {
        if let Err(e) = execute_block_importer(executor, miner, backlog_rx) {
            tracing::error!(parent: None, reason = ?e, "'block-importer' task failed");
        }
    });

    block_importer
        .join()
        .expect("'block-importer' thread panic'ed instead of properly returning an error");

    // Explicitly block the `main` thread while waiting for the storage to drop.
    drop(storage);

    Ok(())
}

// -----------------------------------------------------------------------------
// Block importer
// -----------------------------------------------------------------------------
fn execute_block_importer(
    // services
    executor: Arc<Executor>,
    miner: Arc<Miner>,
    // data
    mut backlog_rx: mpsc::Receiver<BacklogTask>,
) -> anyhow::Result<()> {
    const TASK_NAME: &str = "external-block-executor";
    let _timer = DropTimer::start("importer-offline::execute_block_importer");

    // receives blocks and receipts from the backlog to reexecute and import
    loop {
        if GlobalState::is_shutdown_warn(TASK_NAME) {
            return Ok(());
        };

        // receive blocks to execute
        let Some(blocks) = backlog_rx.blocking_recv() else {
            tracing::info!(parent: None, "{} has no more blocks to reexecute", TASK_NAME);
            return Ok(());
        };

        // ensure range is not empty
        let (Some((block_start, _)), Some((block_end, _))) = (blocks.first(), blocks.last()) else {
            let message = GlobalState::shutdown_from(TASK_NAME, "received empty block range to reexecute");
            return log_and_err!(message);
        };

        // track operation
        let block_start = block_start.number();
        let block_end = block_end.number();
        let batch_blocks_len = blocks.len();
        let receipts_len = blocks.iter().map(|(_, receipts)| receipts.len()).sum::<usize>();
        tracing::info!(parent: None, %block_start, %block_end, %receipts_len, "reexecuting blocks");

        // ensure block range have no gaps
        if block_start.count_to(block_end) != batch_blocks_len as u64 {
            let message = GlobalState::shutdown_from(TASK_NAME, "received block range with gaps to reexecute");
            return log_and_err!(message);
        }

        // imports block transactions
        let mut batch_tx_len = 0;

        let before_block = Instant::now();
        for (mut block, receipts) in blocks.into_iter() {
            if GlobalState::is_shutdown_warn(TASK_NAME) {
                return Ok(());
            }

            // fill missing transaction_type with `v`
            block.transactions.iter_mut().for_each(ExternalTransaction::fill_missing_transaction_type);

            // re-execute (and import) block
            batch_tx_len += block.transactions.len();
            executor.execute_external_block(block.clone(), ExternalReceipts::from(receipts))?;

            // mine and save block
            miner.mine_external_and_commit(block)?;
        }
        let batch_duration = before_block.elapsed();

        let (tps, bpm) = calculate_tps_and_bpm(batch_duration, batch_tx_len, batch_blocks_len);

        tracing::info!(
            parent: None,
            tps,
            blocks_per_minute = format_args!("{bpm:.2}"),
            ?batch_duration,
            %block_start,
            %block_end,
            %receipts_len,
            "reexecuted blocks batch",
        );
    }
}

// -----------------------------------------------------------------------------
// Block loader
// -----------------------------------------------------------------------------
async fn execute_external_rpc_storage_loader(
    // services
    rpc_storage: Arc<dyn ExternalRpcStorage>,
    // data
    blocks_by_fetch: usize,
    paralellism: usize,
    mut start: BlockNumber,
    end: BlockNumber,
    backlog: mpsc::Sender<BacklogTask>,
) -> anyhow::Result<()> {
    const TASK_NAME: &str = "external-block-loader";
    tracing::info!(parent: None, %start, %end, "creating task {}", TASK_NAME);

    // prepare loads to be executed in parallel
    let mut tasks = Vec::new();
    while start <= end {
        let end = min(start + (blocks_by_fetch - 1), end);

        let task = load_blocks_and_receipts(Arc::clone(&rpc_storage), start, end);
        tasks.push(task);

        start += blocks_by_fetch;
    }

    // execute loads in parallel
    let mut tasks = futures::stream::iter(tasks).buffered(paralellism);
    loop {
        if GlobalState::is_shutdown_warn(TASK_NAME) {
            return Ok(());
        };

        // retrieve next batch of loaded blocks
        // if finished, do not cancel, it is expected to finish
        let Some(result) = tasks.next().await else {
            tracing::info!(parent: None, "{} has no more blocks to fetch", TASK_NAME);
            return Ok(());
        };

        // check if executed correctly
        let blocks = match result {
            Ok(blocks) => blocks,
            Err(e) => {
                let message = GlobalState::shutdown_from(TASK_NAME, "failed to fetch block or receipt");
                return log_and_err!(reason = e, message);
            }
        };

        // check blocks were really loaded
        if blocks.is_empty() {
            let message = GlobalState::shutdown_from(TASK_NAME, "no blocks returned when they were expected");
            return log_and_err!(message);
        }

        // send to backlog
        if backlog.send(blocks).await.is_err() {
            return Err(anyhow!(GlobalState::shutdown_from(TASK_NAME, "failed to send task to importer")));
        };
    }
}

async fn load_blocks_and_receipts(rpc_storage: Arc<dyn ExternalRpcStorage>, block_start: BlockNumber, block_end: BlockNumber) -> anyhow::Result<BacklogTask> {
    tracing::info!(parent: None, %block_start, %block_end, "loading blocks and receipts");
    rpc_storage.read_block_and_receipts_in_range(block_start, block_end).await
}

// Finds the block number to stop the import job.
async fn block_number_to_stop(rpc_storage: &Arc<dyn ExternalRpcStorage>) -> anyhow::Result<BlockNumber> {
    match rpc_storage.read_max_block_number_in_range(BlockNumber::ZERO, BlockNumber::MAX).await {
        Ok(Some(number)) => Ok(number),
        Ok(None) => Ok(BlockNumber::ZERO),
        Err(e) => Err(e),
    }
}
