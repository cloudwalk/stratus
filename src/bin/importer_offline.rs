use std::cmp::min;
use std::fs;
use std::sync::Arc;
use std::thread;

use anyhow::anyhow;
use futures::try_join;
use futures::StreamExt;
use itertools::Itertools;
use stratus::config::ImporterOfflineConfig;
use stratus::eth::primitives::Block;
use stratus::eth::primitives::BlockNumber;
use stratus::eth::primitives::BlockSelection;
use stratus::eth::primitives::ExternalBlock;
use stratus::eth::primitives::ExternalReceipt;
use stratus::eth::primitives::ExternalReceipts;
use stratus::eth::storage::CsvExporter;
use stratus::eth::storage::ExternalRpcStorage;
use stratus::eth::storage::InMemoryPermanentStorage;
use stratus::eth::storage::StratusStorage;
use stratus::eth::BlockMiner;
use stratus::eth::Executor;
use stratus::ext::not;
use stratus::GlobalServices;
use stratus::GlobalState;
use tokio::runtime::Handle;
use tokio::sync::mpsc;

/// Number of tasks in the backlog. Each task contains 10_000 blocks and all receipts for them.
const BACKLOG_SIZE: usize = 50;

/// Number of blocks processed in memory before data is flushed to temporary storage and CSV files.
const FLUSH_INTERVAL_IN_BLOCKS: u64 = 100;

/// The maximum amount of blocks in each CSV chunk file.
const CSV_CHUNKING_BLOCKS_INTERVAL: u64 = 2_000_000;

type BacklogTask = (Vec<ExternalBlock>, Vec<ExternalReceipt>);

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<ImporterOfflineConfig>::init()?;
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: ImporterOfflineConfig) -> anyhow::Result<()> {
    // init services
    let rpc_storage = config.rpc_storage.init().await?;
    let storage = config.storage.init().await?;
    let miner = config.miner.init_external_mode(Arc::clone(&storage), None).await?;
    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner), None, None).await;

    // init block snapshots to export
    let block_snapshots = config.export_snapshot.into_iter().map_into().collect();

    // init block range
    let block_start = match config.block_start {
        Some(start) => BlockNumber::from(start),
        None => block_number_to_start(&storage).await?,
    };
    let block_end = match config.block_end {
        Some(end) => BlockNumber::from(end),
        None => block_number_to_stop(&rpc_storage).await?,
    };

    // init csv
    let mut csv = if config.export_csv { Some(CsvExporter::new(block_start)?) } else { None };

    // init shared data between importer and external rpc storage loader
    let (backlog_tx, backlog_rx) = mpsc::channel::<BacklogTask>(BACKLOG_SIZE);

    // load genesis accounts
    let initial_accounts = rpc_storage.read_initial_accounts().await?;
    storage.save_accounts(initial_accounts.clone()).await?;
    if let Some(ref mut csv) = csv {
        if csv.is_accounts_empty() {
            csv.export_initial_accounts(initial_accounts.clone())?;
        }
    }

    // execute thread: external rpc storage loader
    let storage_thread = thread::Builder::new().name("storage-loader".into());
    let storage_tokio = Handle::current();
    let _ = storage_thread.spawn(move || {
        let _tokio_guard = storage_tokio.enter();

        let result = storage_tokio.block_on(execute_external_rpc_storage_loader(
            rpc_storage,
            config.blocks_by_fetch,
            config.paralellism,
            block_start,
            block_end,
            backlog_tx,
        ));
        if let Err(e) = result {
            tracing::error!(reason = ?e, "storage-loader failed");
        }
    });

    // execute thread: block importer
    let importer_thread = thread::Builder::new().name("block-importer".into());
    let importer_tokio = Handle::current();
    let importer_join = importer_thread.spawn(move || {
        let _tokio_guard = importer_tokio.enter();
        let result = importer_tokio.block_on(execute_block_importer(executor, miner, storage, csv, backlog_rx, block_snapshots));
        if let Err(e) = result {
            tracing::error!(reason = ?e, "block-importer failed");
        }
    })?;

    let _ = importer_join.join();

    Ok(())
}

// -----------------------------------------------------------------------------
// Block importer
// -----------------------------------------------------------------------------
#[tracing::instrument(name = "block_importer", skip_all)]
async fn execute_block_importer(
    // services
    executor: Arc<Executor>,
    miner: Arc<BlockMiner>,
    storage: Arc<StratusStorage>,
    mut csv: Option<CsvExporter>,
    // data
    mut backlog_rx: mpsc::Receiver<BacklogTask>,
    blocks_to_export_snapshot: Vec<BlockNumber>,
) -> anyhow::Result<()> {
    const TASK_NAME: &str = "external-block-executor";
    tracing::info!("starting {}", TASK_NAME);

    loop {
        if GlobalState::warn_if_shutdown(TASK_NAME) {
            return Ok(());
        };

        // retrieve new tasks to execute or exit
        let Some((blocks, receipts)) = backlog_rx.recv().await else {
            tracing::info!("{} has no more blocks to process", TASK_NAME);
            return Ok(());
        };

        // imports block transactions
        let block_start = blocks.first().unwrap().number();
        let block_end = blocks.last().unwrap().number();
        let block_last_index = blocks.len() - 1;
        let receipts = ExternalReceipts::from(receipts);

        tracing::info!(%block_start, %block_end, receipts = %receipts.len(), "importing blocks");
        for (block_index, block) in blocks.into_iter().enumerate() {
            if GlobalState::warn_if_shutdown(TASK_NAME) {
                return Ok(());
            }

            // re-execute block
            executor.reexecute_external(&block, &receipts).await?;

            // mine block
            let mined_block = miner.mine_external().await?;

            // export to csv OR permanent storage
            match csv {
                Some(ref mut csv) => import_external_to_csv(&storage, csv, mined_block.clone(), block_index, block_last_index).await?,
                None => miner.commit(mined_block.clone()).await?,
            };

            // export snapshot for tests
            if blocks_to_export_snapshot.contains(mined_block.number()) {
                export_snapshot(&block, &receipts, &mined_block)?;
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Block loader
// -----------------------------------------------------------------------------
#[tracing::instrument(name = "storage_loader", skip_all, fields(start, end, block_by_fetch))]
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
    tracing::info!(%start, %end, "starting {}", TASK_NAME);

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
        if GlobalState::warn_if_shutdown(TASK_NAME) {
            return Ok(());
        };

        // retrieve next batch of loaded blocks
        // if finished, do not cancel, it is expected to finish
        let Some(result) = tasks.next().await else {
            tracing::info!("{} has no more blocks to process", TASK_NAME);
            return Ok(());
        };

        // check if executed correctly
        let Ok((blocks, receipts)) = result else {
            return Err(anyhow!(GlobalState::shutdown_from(TASK_NAME, "failed to fetch block or receipt")));
        };

        // check blocks were really loaded
        if blocks.is_empty() {
            return Err(anyhow!(GlobalState::shutdown_from(TASK_NAME, "no blocks returned when they were expected")));
        }

        // send to backlog
        if backlog.send((blocks, receipts)).await.is_err() {
            return Err(anyhow!(GlobalState::shutdown_from(TASK_NAME, "failed to send task to importer")));
        };
    }
}

async fn load_blocks_and_receipts(rpc_storage: Arc<dyn ExternalRpcStorage>, start: BlockNumber, end: BlockNumber) -> anyhow::Result<BacklogTask> {
    tracing::info!(%start, %end, "retrieving blocks and receipts");
    let blocks_task = rpc_storage.read_blocks_in_range(start, end);
    let receipts_task = rpc_storage.read_receipts_in_range(start, end);
    try_join!(blocks_task, receipts_task)
}

// Finds the block number to start the import job.
async fn block_number_to_start(storage: &StratusStorage) -> anyhow::Result<BlockNumber> {
    // when has an active number, resume from it because it was not imported yet.
    let active_number = storage.read_active_block_number().await?;
    if let Some(active_number) = active_number {
        return Ok(active_number);
    }

    // fallback to last mined block
    // if mined is zero, we need to check if we have the zero block or not to decide if we start from zero or the next.
    // if mined is not zero, then can assume it is the next number after it.
    let mut mined_number = storage.read_mined_block_number().await?;
    let zero_block = storage.read_block(&BlockSelection::Number(BlockNumber::ZERO)).await?;
    if not(mined_number.is_zero()) || zero_block.is_some() {
        mined_number = mined_number.next();
    }
    Ok(mined_number)
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
    fs::write(format!("{}/block.json", dir), serde_json::to_string_pretty(external_block)?)?;
    fs::write(format!("{}/receipts.json", dir), serde_json::to_string_pretty(&receipts_snapshot)?)?;
    fs::write(format!("{}/snapshot.json", dir), serde_json::to_string_pretty(&state_snapshot)?)?;

    Ok(())
}

// -----------------------------------------------------------------------------
// Csv exporter
// -----------------------------------------------------------------------------
async fn import_external_to_csv(
    // services
    storage: &StratusStorage,
    csv: &mut CsvExporter,
    // data
    block: Block,
    block_index: usize,
    block_last_index: usize,
) -> anyhow::Result<()> {
    // export block to csv
    let number = *block.number();
    csv.add_block(block)?;

    let is_last_block = block_index == block_last_index;
    let is_chunk_interval_end = number.as_u64() % CSV_CHUNKING_BLOCKS_INTERVAL == 0;
    let is_flush_interval_end = number.as_u64() % FLUSH_INTERVAL_IN_BLOCKS == 0;

    // check if should flush
    let should_chunk_csv_files = is_chunk_interval_end;
    let should_flush = is_flush_interval_end || is_last_block || should_chunk_csv_files;

    // flush
    if should_flush {
        csv.flush()?;
        storage.flush().await?;
    }

    // chunk
    if should_chunk_csv_files {
        tracing::info!("Chunk ended at block number {number}, starting next CSV chunks for the next block");
        csv.finish_current_chunks(number)?;
    }

    Ok(())
}
