//! Importer-Offline binary.
//!
//! It fetches blocks (and receipts) from an external RPC server, or from a PostgreSQL DB
//! that was prepared with the `rpc-downloader` binary.
//!
//! This importer will check on startup what is the `block_end` value at the external
//! storage, and will not update while running, in contrast with that, the
//! Importer-Online (other binary) will stay up to date with the newer blocks that
//! arrive.

use std::cmp::min;
use std::sync::mpsc;
use std::sync::Arc;

use anyhow::anyhow;
use futures::StreamExt;
use itertools::Itertools;
use stratus::config::ImporterOfflineConfig;
use stratus::eth::executor::Executor;
use stratus::eth::miner::Miner;
use stratus::eth::miner::MinerMode;
use stratus::eth::primitives::Block;
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
use tokio::sync::mpsc as async_mpsc;
use tokio::time::Instant;

#[cfg(all(not(target_env = "msvc"), any(feature = "jemalloc", feature = "jeprof")))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

/// Number of fetcher tasks buffered. Each task contains `--blocks-by-fetch` blocks and all receipts for them
const RPC_FETCHER_CHANNEL_CAPACITY: usize = 10;

/// Size of the executed block batches to be save.
///
/// We want to persist to the storage in batches, this means we don't save a
/// block right away, but the information from that block still needs to be
/// found in the storage.
///
/// By using a channel with capacity 0 to send STORAGE_SAVER_BATCH_SIZE blocks,
/// we guarantee that accounts and slots can either be found in the permanent
/// storage, or in the temporary one.
///
/// In other words, at most (STORAGE_SAVER_BATCH_SIZE) * 2 executed blocks
/// won't be found in the permanent storage, but that's still within the
/// temporary storage capacity.
///
/// We use half because we want parallelism in execution and persisting, both
/// places need to hold blocks that aren't in the permanent storage yet, it's
/// half for each.
const STORAGE_SAVER_BATCH_SIZE: usize = 64 / 2 - 1;

type BlocksToExecute = Vec<ExternalBlockWithReceipts>;
type BlocksToSave = Vec<Block>;

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

    // send blocks from fetcher task to executor task
    let (fetch_to_execute_tx, fetch_to_execute_rx) = async_mpsc::channel::<BlocksToExecute>(RPC_FETCHER_CHANNEL_CAPACITY);

    // send blocks from executor task to saver task
    let (execute_to_save_tx, execute_to_save_rx) = mpsc::sync_channel::<BlocksToSave>(0);

    // load genesis accounts
    let initial_accounts = rpc_storage.read_initial_accounts().await?;
    storage.save_accounts(initial_accounts.clone())?;

    let block_fetcher_fut = run_rpc_block_fetcher(
        rpc_storage,
        config.blocks_by_fetch,
        config.paralellism,
        block_start,
        block_end,
        fetch_to_execute_tx,
    );
    spawn_named("block_fetcher", async {
        if let Err(e) = block_fetcher_fut.await {
            tracing::error!(reason = ?e, "'block-fetcher' task failed");
        }
    });

    let miner_clone = Arc::clone(&miner);
    spawn_thread("block-executor", || {
        if let Err(e) = run_external_block_executor(executor, miner_clone, fetch_to_execute_rx, execute_to_save_tx) {
            tracing::error!(reason = ?e, "'block-executor' task failed");
        }
    });

    let block_saver_handle = spawn_thread("block-saver", || {
        if let Err(e) = run_block_saver(miner, execute_to_save_rx) {
            tracing::error!(reason = ?e, "'block-saver' task failed");
        }
    });

    if let Err(e) = block_saver_handle.join() {
        tracing::error!(reason = ?e, "'block-importer' thread panic'ed");
    }

    // Explicitly block the `main` thread while waiting for the storage to drop.
    drop(storage);

    Ok(())
}

async fn run_rpc_block_fetcher(
    rpc_storage: Arc<dyn ExternalRpcStorage>,
    blocks_by_fetch: usize,
    paralellism: usize,
    mut start: BlockNumber,
    end: BlockNumber,
    to_execute_tx: async_mpsc::Sender<BlocksToExecute>,
) -> anyhow::Result<()> {
    const TASK_NAME: &str = "block-fetcher";

    let mut fetch_stream = {
        // prepare fetches to be executed in parallel
        let mut tasks = Vec::new();
        while start <= end {
            let end = min(start + (blocks_by_fetch - 1), end);

            let task = fetch_blocks_and_receipts(Arc::clone(&rpc_storage), start, end);
            tasks.push(task);

            start += blocks_by_fetch;
        }

        futures::stream::iter(tasks).buffered(paralellism)
    };

    loop {
        if GlobalState::is_shutdown_warn(TASK_NAME) {
            return Ok(());
        };

        // retrieve next batch of fetched blocks or finish task
        let Some(result) = fetch_stream.next().await else {
            tracing::info!(parent: None, "{} has no more blocks to fetch", TASK_NAME);
            return Ok(());
        };

        let blocks = match result {
            Ok(blocks) => blocks,
            Err(e) => {
                return log_and_err!(reason = e, GlobalState::shutdown_from(TASK_NAME, "failed to fetch block or receipt"));
            }
        };

        if blocks.is_empty() {
            return log_and_err!(GlobalState::shutdown_from(TASK_NAME, "no blocks returned when they were expected"));
        }

        if to_execute_tx.send(blocks).await.is_err() {
            return Err(anyhow!(GlobalState::shutdown_from(TASK_NAME, "failed to send task to importer")));
        };
    }
}

fn run_external_block_executor(
    executor: Arc<Executor>,
    miner: Arc<Miner>,
    mut from_fetcher_rx: async_mpsc::Receiver<BlocksToExecute>,
    to_saver_tx: mpsc::SyncSender<BlocksToSave>,
) -> anyhow::Result<()> {
    const TASK_NAME: &str = "run_external_block_executor";
    let _timer = DropTimer::start("importer-offline::run_external_block_executor");

    loop {
        if GlobalState::is_shutdown_warn(TASK_NAME) {
            return Ok(());
        };

        let Some(blocks) = from_fetcher_rx.blocking_recv() else {
            tracing::info!(parent: None, "{} has no more blocks to execute", TASK_NAME);
            return Ok(());
        };

        let (Some((block_start, _)), Some((block_end, _))) = (blocks.first(), blocks.last()) else {
            return log_and_err!(GlobalState::shutdown_from(TASK_NAME, "received empty block range to reexecute"));
        };

        let block_start = block_start.number();
        let block_end = block_end.number();
        let receipts_count = blocks.iter().map(|(_, receipts)| receipts.len()).sum::<usize>();
        let tx_count = blocks.iter().map(|(block, _)| block.transactions.len()).sum();
        let blocks_count = blocks.len();

        tracing::info!(parent: None, %block_start, %block_end, %tx_count, "executing blocks");

        if receipts_count != tx_count {
            return log_and_err!(GlobalState::shutdown_from(TASK_NAME, "receipt count doesn't match transaction count"));
        }

        if block_start.count_to(block_end) != blocks_count as u64 {
            return log_and_err!(GlobalState::shutdown_from(TASK_NAME, "received block range with gaps to execute"));
        }

        let instant_before_execution = Instant::now();

        for blocks in Itertools::chunks(blocks.into_iter(), STORAGE_SAVER_BATCH_SIZE).into_iter() {
            let mut executed_batch = Vec::with_capacity(STORAGE_SAVER_BATCH_SIZE);

            for (mut block, receipts) in blocks {
                if GlobalState::is_shutdown_warn(TASK_NAME) {
                    return Ok(());
                }

                // fill missing transaction_type with `v`
                block.transactions.iter_mut().for_each(ExternalTransaction::fill_missing_transaction_type);

                // TODO: remove clone
                executor.execute_external_block(block.clone(), ExternalReceipts::from(receipts))?;
                let mined_block = miner.mine_external(block)?;
                executed_batch.push(mined_block);
            }

            if to_saver_tx.send(executed_batch).is_err() {
                return log_and_err!(GlobalState::shutdown_from(TASK_NAME, "failed to send executed batch to be saved on storage"));
            }
        }

        let execution_duration = instant_before_execution.elapsed();
        let (tps, bpm) = calculate_tps_and_bpm(execution_duration, tx_count, blocks_count);

        tracing::info!(
            parent: None,
            tps,
            blocks_per_minute = format_args!("{bpm:.2}"),
            ?execution_duration,
            %block_start,
            %block_end,
            %receipts_count,
            "executed blocks batch",
        );
    }
}

fn run_block_saver(miner: Arc<Miner>, from_executor_rx: mpsc::Receiver<BlocksToSave>) -> anyhow::Result<()> {
    const TASK_NAME: &str = "block-saver";
    let _timer = DropTimer::start("importer-offline::run_block_saver");

    loop {
        if GlobalState::is_shutdown_warn(TASK_NAME) {
            return Ok(());
        };

        let Ok(blocks_batch) = from_executor_rx.recv() else {
            tracing::info!("{} has no more batches to save", TASK_NAME);
            return Ok(());
        };

        for block in blocks_batch {
            miner.commit(block)?;
        }
    }
}

async fn fetch_blocks_and_receipts(
    rpc_storage: Arc<dyn ExternalRpcStorage>,
    block_start: BlockNumber,
    block_end: BlockNumber,
) -> anyhow::Result<BlocksToExecute> {
    tracing::info!(parent: None, %block_start, %block_end, "fetching blocks and receipts");
    rpc_storage.read_block_and_receipts_in_range(block_start, block_end).await
}

async fn block_number_to_stop(rpc_storage: &Arc<dyn ExternalRpcStorage>) -> anyhow::Result<BlockNumber> {
    match rpc_storage.read_max_block_number_in_range(BlockNumber::ZERO, BlockNumber::MAX).await {
        Ok(Some(number)) => Ok(number),
        Ok(None) => Ok(BlockNumber::ZERO),
        Err(e) => Err(e),
    }
}
