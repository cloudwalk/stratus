use std::cmp::min;
use std::fs;
use std::sync::Arc;
use std::thread;

use anyhow::anyhow;
use futures::try_join;
use futures::StreamExt;
use itertools::Itertools;
use stratus::channel_read;
use stratus::config::ImporterOfflineConfig;
use stratus::eth::primitives::Block;
use stratus::eth::primitives::BlockNumber;
use stratus::eth::primitives::ExternalBlock;
use stratus::eth::primitives::ExternalReceipt;
use stratus::eth::primitives::ExternalReceipts;
use stratus::eth::storage::ExternalRpcStorage;
use stratus::eth::storage::InMemoryPermanentStorage;
use stratus::eth::BlockMiner;
use stratus::eth::Executor;
use stratus::ext::ResultExt;
use stratus::infra::tracing::info_task_spawn;
use stratus::log_and_err;
use stratus::utils::calculate_tps_and_bpm;
use stratus::utils::DropTimer;
use stratus::GlobalServices;
use stratus::GlobalState;
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::time::Instant;

/// Number of tasks in the backlog. Each task contains 10_000 blocks and all receipts for them.
const BACKLOG_SIZE: usize = 50;

type BacklogTask = (Vec<ExternalBlock>, Vec<ExternalReceipt>);

fn main() -> anyhow::Result<()> {
    let global_services = GlobalServices::<ImporterOfflineConfig>::init();
    global_services.runtime.block_on(run(global_services.config))
}

async fn run(config: ImporterOfflineConfig) -> anyhow::Result<()> {
    let _timer = DropTimer::start("importer-offline");

    // init services
    let rpc_storage = config.rpc_storage.init().await?;
    let storage = config.storage.init().await?;
    let miner = config.miner.init_external_mode(Arc::clone(&storage), None).await?;
    let executor = config.executor.init(Arc::clone(&storage), Arc::clone(&miner)).await;

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

    // init shared data between importer and external rpc storage loader
    let (backlog_tx, backlog_rx) = mpsc::channel::<BacklogTask>(BACKLOG_SIZE);

    // load genesis accounts
    let initial_accounts = rpc_storage.read_initial_accounts().await?;
    storage.save_accounts(initial_accounts.clone())?;

    // execute thread: external rpc storage loader
    let storage_thread = thread::Builder::new().name("storage-loader".into());
    let storage_tokio = Handle::current();

    info_task_spawn("storage-loader");
    let storage_loader_thread = storage_thread
        .spawn(move || {
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
        })
        .expect("spawning storage-loader thread should not fail");

    // execute thread: block importer
    let importer_thread = thread::Builder::new().name("block-importer".into());
    let importer_tokio = Handle::current();

    info_task_spawn("block-importer");
    let block_importer_thread = importer_thread
        .spawn(move || {
            let _tokio_guard = importer_tokio.enter();
            let result = importer_tokio.block_on(execute_block_importer(executor, miner, backlog_rx, block_snapshots));
            if let Err(e) = result {
                tracing::error!(reason = ?e, "block-importer failed");
            }
        })
        .expect("spawning block-importer thread should not fail");

    // await tasks
    if let Err(e) = block_importer_thread.join() {
        tracing::error!(reason = ?e, "block-importer thread failed");
    }
    if let Err(e) = storage_loader_thread.join() {
        tracing::error!(reason = ?e, "storage-loader thread failed");
    }

    Ok(())
}

// -----------------------------------------------------------------------------
// Block importer
// -----------------------------------------------------------------------------
async fn execute_block_importer(
    // services
    executor: Arc<Executor>,
    miner: Arc<BlockMiner>,
    // data
    mut backlog_rx: mpsc::Receiver<BacklogTask>,
    blocks_to_export_snapshot: Vec<BlockNumber>,
) -> anyhow::Result<()> {
    const TASK_NAME: &str = "external-block-executor";
    let _timer = DropTimer::start("importer-offline::execute_block_importer");

    // receives blocks and receipts from the backlog to reexecute and import
    loop {
        if GlobalState::warn_if_shutdown(TASK_NAME) {
            return Ok(());
        };

        // receive new tasks to execute, or exit
        let Some((blocks, receipts)) = channel_read!(backlog_rx) else {
            tracing::info!("{} has no more blocks to process", TASK_NAME);
            return Ok(());
        };

        // imports block transactions
        let block_start = blocks.first().unwrap().number();
        let block_end = blocks.last().unwrap().number();
        let blocks_len = blocks.len();
        let receipts = ExternalReceipts::from(receipts);

        tracing::info!(%block_start, %block_end, receipts = %receipts.len(), "reexecuting (and importing) blocks");
        let mut transaction_count = 0;
        let instant_before_execution = Instant::now();

        for block in blocks.into_iter() {
            if GlobalState::warn_if_shutdown(TASK_NAME) {
                return Ok(());
            }

            // re-execute (and import) block
            executor.external_block(&block, &receipts).await?;
            transaction_count += block.transactions.len();

            // mine and save block
            let mined_block = miner.mine_external()?;
            if blocks_to_export_snapshot.contains(&mined_block.number()) {
                export_snapshot(&block, &receipts, &mined_block)?;
            }
            miner.commit(mined_block.clone())?;
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

        // send to backlog
        if backlog.send((blocks, receipts)).await.is_err() {
            return Err(anyhow!(GlobalState::shutdown_from(TASK_NAME, "failed to send task to importer")));
        };
    }
}

async fn load_blocks_and_receipts(rpc_storage: Arc<dyn ExternalRpcStorage>, start: BlockNumber, end: BlockNumber) -> anyhow::Result<BacklogTask> {
    tracing::info!(%start, %end, "loading blocks and receipts");
    let blocks_task = rpc_storage.read_blocks_in_range(start, end);
    let receipts_task = rpc_storage.read_receipts_in_range(start, end);
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
    fs::write(format!("{}/block.json", dir), serde_json::to_string_pretty(external_block).expect_infallible())?;
    fs::write(
        format!("{}/receipts.json", dir),
        serde_json::to_string_pretty(&receipts_snapshot).expect_infallible(),
    )?;
    fs::write(
        format!("{}/snapshot.json", dir),
        serde_json::to_string_pretty(&state_snapshot).expect_infallible(),
    )?;

    Ok(())
}
