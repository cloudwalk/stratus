use std::cmp::min;
use std::fs;
use std::sync::Arc;

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
use stratus::eth::EthExecutor;
use stratus::ext::not;
#[cfg(feature = "metrics")]
use stratus::infra::metrics;
use stratus::init_global_services;
use stratus::log_and_err;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Number of tasks in the backlog. Each task contains 10_000 blocks and all receipts for them.
const BACKLOG_SIZE: usize = 50;

/// Number of blocks processed in memory before data is flushed to temporary storage and CSV files.
const FLUSH_INTERVAL_IN_BLOCKS: u64 = 100;

/// The maximum amount of blocks in each CSV chunk file.
const CSV_CHUNKING_BLOCKS_INTERVAL: u64 = 2_000_000;

type BacklogTask = (Vec<ExternalBlock>, Vec<ExternalReceipt>);

fn main() -> anyhow::Result<()> {
    let config: ImporterOfflineConfig = init_global_services();
    let runtime = config.init_runtime();
    runtime.block_on(run(config))
}

async fn run(config: ImporterOfflineConfig) -> anyhow::Result<()> {
    // init services
    let rpc_storage = config.rpc_storage.init().await?;
    let stratus_storage = config.stratus_storage.init().await?;
    let executor = config.executor.init(Arc::clone(&stratus_storage));

    // init block range
    let block_start = match config.block_start {
        Some(start) => BlockNumber::from(start),
        None => block_number_to_start(&stratus_storage).await?,
    };
    let block_end = match config.block_end {
        Some(end) => BlockNumber::from(end),
        None => block_number_to_stop(&rpc_storage).await?,
    };

    // init csv
    let mut csv = if config.export_csv { Some(CsvExporter::new(block_start)?) } else { None };

    // init shared data between importer and external rpc storage loader
    let (backlog_tx, backlog_rx) = mpsc::channel::<BacklogTask>(BACKLOG_SIZE);
    let cancellation = CancellationToken::new();
    signal_handler(cancellation.clone());

    // load genesis accounts
    let initial_accounts = rpc_storage.read_initial_accounts().await?;
    stratus_storage.save_accounts_to_perm(initial_accounts.clone()).await?;
    if let Some(ref mut csv) = csv {
        if csv.is_accounts_empty() {
            csv.export_initial_accounts(initial_accounts.clone())?;
        }
    }

    // execute parallel tasks (external rpc storage loader and block importer)
    let _loader_task = tokio::spawn(execute_external_rpc_storage_loader(
        rpc_storage,
        cancellation.clone(),
        config.blocks_by_fetch,
        config.paralellism,
        block_start,
        block_end,
        backlog_tx,
    ));

    let block_snapshots = config.export_snapshot.into_iter().map_into().collect();
    let importer_task = tokio::spawn(execute_block_importer(
        executor,
        stratus_storage,
        csv,
        cancellation.clone(),
        backlog_rx,
        block_snapshots,
    ));

    importer_task.await??;

    Ok(())
}

fn signal_handler(cancellation: CancellationToken) {
    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                tracing::info!("shutting down");
                cancellation.cancel();
            }

            Err(err) => tracing::error!("Unable to listen for shutdown signal: {}", err),
        }
    });
}

// -----------------------------------------------------------------------------
// Block importer
// -----------------------------------------------------------------------------
async fn execute_block_importer(
    // services
    mut executor: EthExecutor,
    stratus_storage: Arc<StratusStorage>,
    mut csv: Option<CsvExporter>,
    cancellation: CancellationToken,
    // data
    mut backlog_rx: mpsc::Receiver<BacklogTask>,
    blocks_to_export_snapshot: Vec<BlockNumber>,
) -> anyhow::Result<()> {
    tracing::info!("block importer starting");

    // import blocks and transactions in foreground
    let reason = loop {
        // retrieve new tasks to execute
        let Some((blocks, receipts)) = backlog_rx.recv().await else {
            cancellation.cancel();
            break "block loader finished or failed";
        };

        if cancellation.is_cancelled() {
            break "exiting block importer";
        };

        let block_start = blocks.first().unwrap().number();
        let block_end = blocks.last().unwrap().number();
        let block_last_index = blocks.len() - 1;
        let receipts = ExternalReceipts::from(receipts);

        // imports block transactions
        tracing::info!(%block_start, %block_end, receipts = %receipts.len(), "importing blocks");
        for (block_index, block) in blocks.into_iter().enumerate() {
            #[cfg(feature = "metrics")]
            let start = metrics::now();

            // import block
            // * when exporting to csv, permanent state is written to csv
            // * when not exporting to csv, permanent state is written to storage
            let mined_block = match csv {
                Some(ref mut csv) => {
                    let mined_block = executor.import_external_to_temp(block.clone(), &receipts).await?;
                    import_external_to_csv(&stratus_storage, csv, mined_block.clone(), block_index, block_last_index).await?;
                    mined_block
                }
                None => executor.import_external_to_perm(block.clone(), &receipts).await?,
            };

            // export snapshot for tests
            if blocks_to_export_snapshot.contains(&block.number()) {
                export_snapshot(&block, &receipts, &mined_block)?;
            }

            #[cfg(feature = "metrics")]
            metrics::inc_import_offline(start.elapsed());
        }
    };

    tracing::info!(%reason, "block importer finished");
    Ok(())
}

// -----------------------------------------------------------------------------
// Block loader
// -----------------------------------------------------------------------------
async fn execute_external_rpc_storage_loader(
    // services
    rpc_storage: Arc<dyn ExternalRpcStorage>,
    cancellation: CancellationToken,
    // data
    blocks_by_fetch: usize,
    paralellism: usize,
    mut start: BlockNumber,
    end: BlockNumber,
    backlog: mpsc::Sender<BacklogTask>,
) -> anyhow::Result<()> {
    tracing::info!(%start, %end, "external rpc storage loader starting");

    // prepare loads to be executed in parallel
    let mut tasks = Vec::new();
    while start <= end {
        let end = min(start + (blocks_by_fetch - 1), end);
        tasks.push(load_blocks_and_receipts(Arc::clone(&rpc_storage), cancellation.clone(), start, end));
        start += blocks_by_fetch;
    }

    // execute loads in parallel
    let mut tasks = futures::stream::iter(tasks).buffered(paralellism);
    let reason = loop {
        // retrieve next batch of loaded blocks
        let Some(result) = tasks.next().await else {
            cancellation.cancel();
            break "no more blocks to process";
        };

        // check if executed correctly
        let Ok((blocks, receipts)) = result else {
            cancellation.cancel();
            break "block or receipt fetch failed";
        };

        // check blocks were really loaded
        if blocks.is_empty() {
            cancellation.cancel();
            return log_and_err!("no blocks returned when they were expected");
        }

        // send to backlog
        if backlog.send((blocks, receipts)).await.is_err() {
            tracing::error!("failed to send task to importer");
            cancellation.cancel();
            return log_and_err!("failed to send blocks and receipts to importer");
        };
    };

    tracing::info!(%reason, "external rpc storage loader finished");
    Ok(())
}

async fn load_blocks_and_receipts(
    rpc_storage: Arc<dyn ExternalRpcStorage>,
    cancellation: CancellationToken,
    start: BlockNumber,
    end: BlockNumber,
) -> anyhow::Result<BacklogTask> {
    tracing::info!(%start, %end, "retrieving blocks and receipts");
    if cancellation.is_cancelled() {
        return Err(anyhow!("cancelled"));
    }
    let blocks_task = rpc_storage.read_blocks_in_range(start, end);
    let receipts_task = rpc_storage.read_receipts_in_range(start, end);
    try_join!(blocks_task, receipts_task)
}

// Finds the block number to start the import job.
async fn block_number_to_start(stratus_storage: &StratusStorage) -> anyhow::Result<BlockNumber> {
    // when has an active number, resume from it because it was not imported yet.
    let active_number = stratus_storage.read_active_block_number().await?;
    if let Some(active_number) = active_number {
        return Ok(active_number);
    }

    // fallback to last mined block
    // if mined is zero, we need to check if we have the zero block or not to decide if we start from zero or the next.
    // if mined is not zero, then can assume it is the next number after it.
    let mut mined_number = stratus_storage.read_mined_block_number().await?;
    let zero_block = stratus_storage.read_block(&BlockSelection::Number(BlockNumber::ZERO)).await?;
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
    stratus_storage: &StratusStorage,
    csv: &mut CsvExporter,
    // data
    block: Block,
    block_index: usize,
    block_last_index: usize,
) -> anyhow::Result<()> {
    // export block to csv
    let number = *block.number();
    csv.add_block(block)?;

    // check if should flush
    let is_last_block = block_index == block_last_index;
    let is_chunk_interval_end = number.as_u64() % CSV_CHUNKING_BLOCKS_INTERVAL == 0;
    let is_flush_interval_end = number.as_u64() % FLUSH_INTERVAL_IN_BLOCKS == 0;

    let should_chunk_csv_files = is_chunk_interval_end && !is_last_block;
    let should_flush = is_flush_interval_end || is_last_block || should_chunk_csv_files;

    // flush
    if should_flush {
        csv.flush()?;
        stratus_storage.flush_temp().await?;
    }

    // chunk
    if should_chunk_csv_files {
        tracing::info!("Chunk ended at block number {number}, starting next CSV chunks for the next block");
        csv.finish_current_chunks(number)?;
    }

    Ok(())
}
