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
use std::fs;
use std::sync::mpsc;
use std::sync::Arc;

use anyhow::anyhow;
use futures::try_join;
use futures::StreamExt;
use itertools::Itertools;
use stratus::config::ImporterOfflineConfig;
use stratus::eth::executor::Executor;
use stratus::eth::miner::Miner;
use stratus::eth::primitives::Block;
use stratus::eth::primitives::BlockNumber;
use stratus::eth::primitives::ExternalBlock;
use stratus::eth::primitives::ExternalReceipt;
use stratus::eth::primitives::ExternalReceipts;
use stratus::eth::storage::ExternalRpcStorage;
use stratus::eth::storage::InMemoryPermanentStorage;
use stratus::eth::storage::INMEMORY_TEMPORARY_STORAGE_MAX_BLOCKS;
use stratus::ext::spawn_named;
use stratus::ext::spawn_thread;
use stratus::ext::to_json_string_pretty;
use stratus::log_and_err;
use stratus::utils::calculate_tps_and_bpm;
use stratus::utils::DropTimer;
use stratus::GlobalServices;
use stratus::GlobalState;
use tokio::sync::mpsc as async_mpsc;
use tokio::time::Instant;

/// Number of loader tasks buffered. Each task contains `--blocks-by-fetch` blocks and all receipts for them.
const RPC_LOADER_CHANNEL_CAPACITY: usize = 25;

/// Size of the block batches to save after execution, and capacity of the batch channel.
///
/// We are using half of the inmemory temporary storage and a channel with size 1, this way, we can parallize
/// execution and storage saving while staying below the inmemory storage limit.
///
/// By setting channel capacity to 1, we use backpressure to ensure that at most
/// `INMEMORY_TEMPORARY_STORAGE_MAX_BLOCKS` are executed at a time.
///
/// REMOVE THIS: Decrease by one to give room for the pending block.
/// TODO: why `max/2-1` doesn't work?
const STORAGE_WRITER_BATCHES_SIZE: usize = INMEMORY_TEMPORARY_STORAGE_MAX_BLOCKS / 3;
const STORAGE_WRITER_CHANNEL_CAPACITY: usize = 1;

type LoadedBatch = (Vec<ExternalBlock>, Vec<ExternalReceipt>);

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<ImporterOfflineConfig>::init();
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: ImporterOfflineConfig) -> anyhow::Result<()> {
    let _timer = DropTimer::start("importer-offline");

    // init services
    let rpc_storage = config.rpc_storage.init().await?;
    let storage = config.storage.init()?;
    let miner = config.miner.init_external_mode(Arc::clone(&storage))?;
    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner));

    // init block snapshots to export
    let block_snapshots = config.export_snapshot.into_iter().map_into().collect();

    // init block range
    let block_start = match config.block_start {
        Some(start) => BlockNumber::from(start),
        None => storage.read_block_number_to_resume_import()?,
    };
    let block_end = match config.block_end {
        Some(end) => BlockNumber::from(end),
        None => block_number_to_stop(&rpc_storage).await?,
    };

    // init channel that sends batches from external storage loader to block executor
    let (loader_tx, loader_rx) = async_mpsc::channel::<LoadedBatch>(RPC_LOADER_CHANNEL_CAPACITY);

    // init channel that sends batches from executor to the storage writer
    let (writer_batch_tx, writer_batch_rx) = mpsc::sync_channel::<Vec<Block>>(STORAGE_WRITER_CHANNEL_CAPACITY);

    // load genesis accounts
    let initial_accounts = rpc_storage.read_initial_accounts().await?;
    storage.save_accounts(initial_accounts.clone())?;

    let storage_loader_fut = run_external_rpc_storage_loader(rpc_storage, config.blocks_by_fetch, config.paralellism, block_start, block_end, loader_tx);
    let _storage_loader = spawn_named("storage-loader", async move {
        if let Err(err) = storage_loader_fut.await {
            tracing::error!(?err, "'storage-loader' task failed");
        }
    });

    let _block_executor = spawn_thread("block-executor", {
        let miner = Arc::clone(&miner);
        || {
            if let Err(err) = run_external_block_executor(executor, miner, loader_rx, writer_batch_tx, block_snapshots) {
                tracing::error!(?err, "'block-executor' task failed");
            }
        }
    });

    let block_saver = spawn_thread("block-saver", || {
        if let Err(err) = run_block_saver(miner, writer_batch_rx) {
            tracing::error!(?err, "'block-saver' task failed");
        }
    });

    block_saver.join().expect("'block-saver' thread panic'ed instead of returning an error");

    // Explicitly block the `main` thread to drop the storage.
    drop(storage);

    Ok(())
}

// -----------------------------------------------------------------------------
// Block executor
// -----------------------------------------------------------------------------
fn run_external_block_executor(
    executor: Arc<Executor>,
    miner: Arc<Miner>,
    mut loader_rx: async_mpsc::Receiver<LoadedBatch>,
    writer_batch_tx: mpsc::SyncSender<Vec<Block>>,
    blocks_to_export_snapshot: Vec<BlockNumber>,
) -> anyhow::Result<()> {
    const TASK_NAME: &str = "external-block-executor";
    let _timer = DropTimer::start("importer-offline::run_external_block_executor");

    // receives blocks and receipts from the loader to reexecute and import
    loop {
        if GlobalState::is_shutdown_warn(TASK_NAME) {
            return Ok(());
        };

        // receive new tasks to execute, or exit
        let Some((blocks, receipts)) = loader_rx.blocking_recv() else {
            tracing::info!("{} has no more blocks to reexecute", TASK_NAME);
            return Ok(());
        };

        // ensure range is not empty
        let (Some(block_start), Some(block_end)) = (blocks.first(), blocks.last()) else {
            let message = GlobalState::shutdown_from(TASK_NAME, "received empty block range to reexecute");
            return log_and_err!(message);
        };

        // track operation
        let block_start = block_start.number();
        let block_end = block_end.number();
        let blocks_len = blocks.len();
        tracing::info!(%block_start, %block_end, receipts = %receipts.len(), "reexecuting blocks");

        // ensure block range have no gaps
        if block_start.count_to(&block_end) != blocks_len as u64 {
            let message = GlobalState::shutdown_from(TASK_NAME, "received block range with gaps to reexecute");
            return log_and_err!(message);
        }

        // imports block transactions
        let receipts = ExternalReceipts::from(receipts);
        let mut transaction_count = 0;
        let instant_before_execution = Instant::now();

        let mut block_batch = Vec::with_capacity(STORAGE_WRITER_BATCHES_SIZE);
        for block in blocks {
            if GlobalState::is_shutdown_warn(TASK_NAME) {
                return Ok(());
            }

            // re-execute (and import) block
            executor.execute_external_block(&block, &receipts)?;
            transaction_count += block.transactions.len();

            // mine and save block
            let mined_block = miner.mine_external()?;
            if blocks_to_export_snapshot.contains(&mined_block.number()) {
                export_snapshot(&block, &receipts, &mined_block)?;
            }
            block_batch.push(mined_block);

            if block_batch.len() == STORAGE_WRITER_BATCHES_SIZE {
                writer_batch_tx.send(block_batch).unwrap();
                block_batch = vec![];
            }
        }

        let duration = instant_before_execution.elapsed();
        let (tps, bpm) = calculate_tps_and_bpm(duration, transaction_count, blocks_len);

        tracing::info!(
            tps,
            blocks_per_minute = format_args!("{bpm:.2}"),
            ?duration,
            %block_start,
            %block_end,
            receipts = receipts.len(),
            "reexecuted blocks batch",
        );

        // send the leftovers
        if !block_batch.is_empty() {
            writer_batch_tx.send(block_batch).unwrap();
        }
    }
}

// -----------------------------------------------------------------------------
// Block saver
// -----------------------------------------------------------------------------
fn run_block_saver(miner: Arc<Miner>, writer_batch_rx: mpsc::Receiver<Vec<Block>>) -> anyhow::Result<()> {
    const TASK_NAME: &str = "block-saver";
    let _timer = DropTimer::start("importer-offline::run_block_saver");

    // receives blocks and receipts from the loader to reexecute and import
    loop {
        if GlobalState::is_shutdown_warn(TASK_NAME) {
            return Ok(());
        };

        // receive new tasks to execute, or exit
        let Ok(blocks_batch) = writer_batch_rx.recv() else {
            tracing::info!("{} has no more batches to save", TASK_NAME);
            return Ok(());
        };

        for block in blocks_batch {
            miner.commit(block)?;
        }
    }
}

// -----------------------------------------------------------------------------
// Block loader
// -----------------------------------------------------------------------------
async fn run_external_rpc_storage_loader(
    rpc_storage: Arc<dyn ExternalRpcStorage>,
    blocks_by_fetch: usize,
    paralellism: usize,
    mut start: BlockNumber,
    end: BlockNumber,
    loader_tx: async_mpsc::Sender<LoadedBatch>,
) -> anyhow::Result<()> {
    const TASK_NAME: &str = "external-block-loader";
    tracing::info!(%start, %end, "creating task {}", TASK_NAME);

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
            tracing::info!("{} has no more blocks to process", TASK_NAME);
            return Ok(());
        };

        // check if executed correctly
        let (blocks, receipts) = match result {
            Ok((blocks, receipts)) => (blocks, receipts),
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

        // send to channel
        if loader_tx.send((blocks, receipts)).await.is_err() {
            return Err(anyhow!(GlobalState::shutdown_from(TASK_NAME, "failed to send task to executor")));
        };
    }
}

async fn load_blocks_and_receipts(rpc_storage: Arc<dyn ExternalRpcStorage>, block_start: BlockNumber, block_end: BlockNumber) -> anyhow::Result<LoadedBatch> {
    tracing::info!(%block_start, %block_end, "loading blocks and receipts");
    let blocks_task = rpc_storage.read_blocks_in_range(block_start, block_end);
    let receipts_task = rpc_storage.read_receipts_in_range(block_start, block_end);
    try_join!(blocks_task, receipts_task)
}

// Finds the block number to stop the import job.
async fn block_number_to_stop(rpc_storage: &Arc<dyn ExternalRpcStorage>) -> anyhow::Result<BlockNumber> {
    match rpc_storage.read_max_block_number_in_range(BlockNumber::ZERO, BlockNumber::MAX).await {
        Ok(Some(number)) => Ok(number),
        Ok(None) => Ok(BlockNumber::ZERO),
        Err(e) => Err(e),
    }
}

// -----------------------------------------------------------------------------
// Snapshot exporter
// -----------------------------------------------------------------------------
fn export_snapshot(external_block: &ExternalBlock, external_receipts: &ExternalReceipts, mined_block: &Block) -> anyhow::Result<()> {
    // generate snapshot
    let state_snapshot = InMemoryPermanentStorage::dump_snapshot(mined_block.compact_account_changes());
    let receipts_snapshot = external_receipts.filter_block(external_block.number());

    // create dir
    let dir = format!("tests/fixtures/snapshots/{}/", mined_block.number());
    fs::create_dir_all(&dir)?;

    // write json
    fs::write(format!("{}/block.json", dir), to_json_string_pretty(external_block))?;
    fs::write(format!("{}/receipts.json", dir), to_json_string_pretty(&receipts_snapshot))?;
    fs::write(format!("{}/snapshot.json", dir), to_json_string_pretty(&state_snapshot))?;

    Ok(())
}
