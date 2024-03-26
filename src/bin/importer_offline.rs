use std::cmp::min;
use std::sync::Arc;
use std::time::Instant;

use anyhow::anyhow;
use futures::try_join;
use futures::StreamExt;
use stratus::config::ImporterOfflineConfig;
use stratus::eth::primitives::BlockNumber;
use stratus::eth::primitives::BlockSelection;
use stratus::eth::primitives::ExternalBlock;
use stratus::eth::primitives::ExternalReceipt;
use stratus::eth::primitives::ExternalReceipts;
use stratus::eth::storage::CsvExporter;
use stratus::eth::storage::ExternalRpcStorage;
use stratus::eth::storage::StratusStorage;
use stratus::eth::EthExecutor;
use stratus::ext::not;
use stratus::if_else;
use stratus::infra::metrics;
use stratus::init_global_services;
use stratus::log_and_err;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Number of blocks fetched in each query.
const BLOCKS_BY_FETCH: usize = 10_000;

/// Number of tasks in the backlog. Each task contains 10_000 blocks and all receipts for them.
const BACKLOG_SIZE: usize = 50;

/// Number of blocks processed in memory before data is flushed to temporary storage and CSV files.
const FLUSH_INTERVAL_IN_BLOCKS: u64 = 100;

type BacklogTask = (Vec<ExternalBlock>, Vec<ExternalReceipt>);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // init services
    let config: ImporterOfflineConfig = init_global_services();
    let rpc_storage = config.rpc_storage.init().await?;
    let stratus_storage = config.init_stratus_storage().await?;
    let executor = config.init_executor(Arc::clone(&stratus_storage));

    let block_start = match config.start {
        Some(start) => start,
        None => block_number_to_start(&stratus_storage).await?,
    };
    let block_end = match config.end {
        Some(end) => end,
        None => block_number_to_stop(&rpc_storage).await?,
    };

    let mut csv = if_else!(config.export_csv, Some(CsvExporter::new(block_start)?), None);

    // init shared data between importer and external rpc storage loader
    let (backlog_tx, backlog_rx) = mpsc::channel::<BacklogTask>(BACKLOG_SIZE);
    let cancellation = CancellationToken::new();

    // import genesis accounts
    let accounts = rpc_storage.read_initial_accounts().await?;
    if let Some(ref mut csv) = csv {
        for account in accounts.iter() {
            csv.add_initial_account(account.clone())?;
        }
        csv.flush()?;
    }
    stratus_storage.save_accounts_to_perm(accounts).await?;

    // execute parallel tasks (external rpc storage loader and block importer)
    tokio::spawn(execute_external_rpc_storage_loader(
        rpc_storage,
        cancellation.clone(),
        config.paralellism,
        block_start,
        block_end,
        backlog_tx,
    ));
    execute_block_importer(executor, Arc::clone(&stratus_storage), csv, cancellation, backlog_rx).await?;

    Ok(())
}

// -----------------------------------------------------------------------------
// Importer
// -----------------------------------------------------------------------------
async fn execute_block_importer(
    // services
    executor: EthExecutor,
    stratus_storage: Arc<StratusStorage>,
    mut csv: Option<CsvExporter>,
    cancellation: CancellationToken,
    // data
    mut backlog_rx: mpsc::Receiver<BacklogTask>,
) -> anyhow::Result<()> {
    tracing::info!("block importer starting");

    // import blocks and transactions in foreground
    let reason = loop {
        // retrieve new tasks to execute
        let Some((blocks, receipts)) = backlog_rx.recv().await else {
            cancellation.cancel();
            break "block loader finished or failed";
        };

        // imports transactions
        let block_start = blocks.first().unwrap().number();
        let block_end = blocks.last().unwrap().number();
        let mut receipts = ExternalReceipts::from(receipts);

        tracing::info!(%block_start, %block_end, receipts = %receipts.len(), "importing blocks");
        let block_last_index = blocks.len() - 1;
        for (block_index, block) in blocks.into_iter().enumerate() {
            let start = Instant::now();

            match csv {
                // when exporting to csv, only persist temporary changes because permanent will be bulk loaded at the end of the process
                Some(ref mut csv) => {
                    let block = executor.import_external_to_temp(block, &mut receipts).await?;
                    let number = *block.number();
                    csv.add_block(block)?;

                    // flush when reached the specified interval or is the last block in the loop
                    let should_flush = (number.as_u64() % FLUSH_INTERVAL_IN_BLOCKS == 0) || (block_index == block_last_index);
                    if should_flush {
                        csv.flush()?;
                        stratus_storage.flush_temp().await?;
                    }
                }
                // when not exporting to csv, persist the entire block to permanent immediately
                None => {
                    executor.import_external_to_perm(block, &mut receipts).await?;
                }
            }

            metrics::inc_import_offline(start.elapsed());
        }
    };

    tracing::info!(%reason, "block importer finished");
    Ok(())
}

// -----------------------------------------------------------------------------
// External RPC storage loader
// -----------------------------------------------------------------------------
async fn execute_external_rpc_storage_loader(
    // services
    rpc_storage: Arc<dyn ExternalRpcStorage>,
    cancellation: CancellationToken,
    // data
    paralellism: usize,
    mut start: BlockNumber,
    end: BlockNumber,
    backlog: mpsc::Sender<BacklogTask>,
) -> anyhow::Result<()> {
    tracing::info!("external rpc storage loader starting");

    tracing::info!(%start, %end, "block limits");

    // prepare loads to be executed in parallel
    let mut tasks = Vec::new();
    while start <= end {
        let end = min(start + (BLOCKS_BY_FETCH - 1), end);
        tasks.push(load_blocks_and_receipts(Arc::clone(&rpc_storage), cancellation.clone(), start, end));
        start += BLOCKS_BY_FETCH;
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
