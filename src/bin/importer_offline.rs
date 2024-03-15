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

/// Number of tasks in the backlog.
///
/// Each task contains 10_000 blocks and all receipts for them.
const BACKLOG_SIZE: usize = 50;

type BacklogTask = (Vec<ExternalBlock>, Vec<ExternalReceipt>);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // init services
    let config: ImporterOfflineConfig = init_global_services();
    let rpc_storage = config.rpc_storage.init().await?;
    let stratus_storage = config.init_stratus_storage().await?;
    let executor = config.init_executor(Arc::clone(&stratus_storage));
    let mut csv = if_else!(config.export_csv, Some(CsvExporter::new()?), None);

    // init shared data between importer and external rpc storage loader
    let (backlog_tx, backlog_rx) = mpsc::channel::<BacklogTask>(BACKLOG_SIZE);
    let cancellation = CancellationToken::new();

    // import genesis accounts
    let accounts = rpc_storage.read_initial_accounts().await?;
    if let Some(ref mut csv) = csv {
        csv.export_initial_accounts(accounts.clone())?;
    }
    stratus_storage.save_accounts_to_perm(accounts).await?;

    // execute parallel tasks (external rpc storage loader and block importer)
    tokio::spawn(execute_external_rpc_storage_loader(
        rpc_storage,
        stratus_storage,
        cancellation.clone(),
        config.paralellism,
        backlog_tx,
    ));
    execute_block_importer(executor, csv, cancellation, backlog_rx).await?;

    Ok(())
}

// -----------------------------------------------------------------------------
// Importer
// -----------------------------------------------------------------------------
async fn execute_block_importer(
    // services
    executor: EthExecutor,
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
        for block in blocks {
            let start = Instant::now();
            let block = executor.reexecute_external(block, &mut receipts).await?;
            if let Some(ref mut csv) = csv {
                csv.export_block(block)?;
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
    stratus_storage: Arc<StratusStorage>,
    cancellation: CancellationToken,
    // data
    paralellism: usize,
    backlog: mpsc::Sender<BacklogTask>,
) -> anyhow::Result<()> {
    tracing::info!("external rpc storage loader starting");

    // find block limits to load
    let mut start = stratus_storage.read_current_block_number().await?;
    if not(start.is_zero()) || stratus_storage.read_block(&BlockSelection::Number(BlockNumber::ZERO)).await?.is_some() {
        start = start.next();
    };
    let end = match rpc_storage.read_max_block_number_in_range(BlockNumber::ZERO, BlockNumber::MAX).await {
        Ok(Some(number)) => number,
        Ok(None) => BlockNumber::ZERO,
        Err(e) => {
            cancellation.cancel();
            return Err(e);
        }
    };
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
