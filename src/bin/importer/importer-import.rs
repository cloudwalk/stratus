mod _postgres;

use std::collections::HashMap;
use std::sync::Arc;

use _postgres::*;
use itertools::Itertools;
use stratus::config::ImporterImportConfig;
use stratus::eth::primitives::Account;
use stratus::infra::postgres::Postgres;
use stratus::init_global_services;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Number of tasks in backlog: (BACKLOG_SIZE * BacklogTask)
const BACKLOG_SIZE: usize = 10;
type BacklogTask = (Vec<BlockRow>, Vec<ReceiptRow>);

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    // init services
    let config: ImporterImportConfig = init_global_services();
    let pg = Arc::new(Postgres::new(&config.postgres_url).await?);
    let storage = config.init_storage().await?;
    let executor = config.init_executor(Arc::clone(&storage));

    // init shared data between importer and postgres loader
    let (backlog_tx, mut backlog_rx) = mpsc::channel::<BacklogTask>(BACKLOG_SIZE);
    let cancellation = CancellationToken::new();

    // import genesis accounts
    let balances = pg_retrieve_downloaded_balances(&pg).await?;
    let accounts = balances
        .into_iter()
        .map(|row| Account::new_with_balance(row.address, row.balance))
        .collect_vec();
    storage.save_accounts_to_perm(accounts).await?;

    // load blocks and receipts in background
    tokio::spawn(keep_loading_blocks(pg, cancellation.clone(), backlog_tx));

    // import blocks and transactions in foreground
    let reason = loop {
        // retrieve new tasks to execute
        let Some((blocks, receipts)) = backlog_rx.recv().await else {
            cancellation.cancel();
            break "block loader finished or failed";
        };

        // index receipts
        let mut receipts_by_hash = HashMap::with_capacity(receipts.len());
        for receipt in receipts {
            receipts_by_hash.insert(receipt.payload.0.transaction_hash.into(), receipt.payload);
        }

        // imports transactions
        let block_start = blocks.first().unwrap().number;
        let block_end = blocks.last().unwrap().number;
        tracing::info!(%block_start, %block_end, receipts = %receipts_by_hash.len(), "importing blocks");
        for block in blocks {
            executor.import_offline(block.payload, &receipts_by_hash).await?;
        }
    };
    tracing::info!(%reason, "importer finished");

    Ok(())
}

// -----------------------------------------------------------------------------
// Postgres block loader
// -----------------------------------------------------------------------------
async fn keep_loading_blocks(
    // services
    pg: Arc<Postgres>,
    cancellation: CancellationToken,
    // data
    backlog: mpsc::Sender<BacklogTask>,
) -> anyhow::Result<()> {
    let mut tx = pg_init_blocks_cursor(&pg).await?;

    let reason = loop {
        if cancellation.is_cancelled() {
            break "importer finished or failed";
        }

        // find blocks
        tracing::info!("retrieving blocks to process");
        let blocks = match pg_fetch_blocks(&mut tx).await {
            Ok(blocks) =>
                if blocks.is_empty() {
                    cancellation.cancel();
                    break "no more blocks to process";
                } else {
                    blocks
                },
            Err(_) => {
                cancellation.cancel();
                break "error loading blocks";
            }
        };

        // find receipts
        tracing::info!("retrieving receipts to process");
        let block_start = blocks.first().unwrap().number;
        let block_end = blocks.last().unwrap().number;
        let Ok(receipts) = pg_retrieve_downloaded_receipts(&pg, block_start, block_end).await else {
            cancellation.cancel();
            break "error loading receipts";
        };

        // send to backlog
        if backlog.send((blocks, receipts)).await.is_err() {
            cancellation.cancel();
            break "error sending tasks to importer";
        };
    };

    tracing::info!(%reason, "postgres loader finished");
    Ok(())
}
