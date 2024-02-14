#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use itertools::Itertools;
use serde_json::Value as JsonValue;
use sqlx::Row;
use stratus::config::ImporterImportConfig;
use stratus::eth::primitives::Account;
use stratus::eth::primitives::Address;
use stratus::eth::primitives::BlockNumber;
use stratus::eth::primitives::ExternalBlock;
use stratus::eth::primitives::ExternalReceipt;
use stratus::eth::primitives::Nonce;
use stratus::eth::primitives::Wei;
use stratus::infra::postgres::Postgres;
use stratus::init_global_services;
use stratus::log_and_err;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    // init services
    let config: ImporterImportConfig = init_global_services();
    let pg = Arc::new(Postgres::new(&config.postgres_url).await?);
    let storage = config.init_storage().await?;
    let executor = config.init_executor(Arc::clone(&storage));

    // process genesis accounts
    let balances = db_retrieve_balances(&pg).await?;
    let accounts = balances
        .into_iter()
        .map(|balance| Account {
            address: balance.address,
            nonce: Nonce::ZERO,
            balance: balance.balance,
            bytecode: None,
        })
        .collect_vec();
    storage.save_initial_accounts(accounts).await?;

    // process blocks
    let mut tx = db_init_blocks_cursor(&pg).await?;
    loop {
        // find blocks
        let blocks = db_fetch_blocks(&mut tx).await?;
        if blocks.is_empty() {
            tracing::info!("no more blocks to process");
            break;
        }

        // find receipts
        let block_start = blocks.first().unwrap().number;
        let block_end = blocks.last().unwrap().number;
        let receipts = db_retrieve_receipts(&pg, block_start, block_end).await?;

        // index receipts
        let mut receipts_by_hash = HashMap::with_capacity(receipts.len());
        for receipt in receipts {
            receipts_by_hash.insert(receipt.payload.0.transaction_hash.into(), receipt.payload);
        }

        // imports transactions
        tracing::info!(%block_start, %block_end, receipts = %receipts_by_hash.len(), "importing blocks");
        for block in blocks {
            executor.import_offline(block.payload, &receipts_by_hash).await?;
        }
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// Postgres
// -----------------------------------------------------------------------------

struct BlockRow {
    number: i64,
    payload: ExternalBlock,
}

struct ReceiptRow {
    block_number: i64,
    payload: ExternalReceipt,
}

struct BalanceRow {
    address: Address,
    balance: Wei,
}

async fn db_retrieve_balances(pg: &Postgres) -> anyhow::Result<Vec<BalanceRow>> {
    tracing::debug!("retrieving downloaded balances");

    let result = sqlx::query_file_as!(BalanceRow, "src/bin/importer/sql/select_downloaded_balances.sql")
        .fetch_all(&pg.connection_pool)
        .await;
    match result {
        Ok(accounts) => Ok(accounts),
        Err(e) => log_and_err!(reason = e, "failed to retrieve downloaded balances"),
    }
}

async fn db_init_blocks_cursor(pg: &Postgres) -> anyhow::Result<sqlx::Transaction<'_, sqlx::Postgres>> {
    let start = match db_retrieve_max_imported_block(pg).await? {
        Some(number) => number.next(),
        None => BlockNumber::ZERO,
    };
    tracing::info!(%start, "initing blocks cursor");

    let mut tx = pg.start_transaction().await?;
    let result = sqlx::query_file!("src/bin/importer/sql/cursor_declare_downloaded_blocks.sql", start.as_i64())
        .execute(&mut *tx)
        .await;

    match result {
        Ok(_) => Ok(tx),
        Err(e) => log_and_err!(reason = e, "failed to open postgres cursor"),
    }
}

async fn db_fetch_blocks(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) -> anyhow::Result<Vec<BlockRow>> {
    tracing::debug!("fetching more blocks");

    let result = sqlx::query_file_scalar!("src/bin/importer/sql/cursor_fetch_downloaded_blocks.sql")
        .fetch_all(&mut **tx)
        .await;

    match result {
        Ok(rows) => {
            let mut parsed_rows: Vec<BlockRow> = Vec::with_capacity(rows.len());
            for row in rows {
                let parsed = BlockRow {
                    number: row.get_unchecked::<'_, i64, usize>(0),
                    payload: row.get_unchecked::<'_, JsonValue, usize>(1).try_into()?,
                };
                parsed_rows.push(parsed);
            }
            Ok(parsed_rows)
        }
        Err(e) => log_and_err!(reason = e, "failed to fetch blocks from cursor"),
    }
}

async fn db_retrieve_receipts(pg: &Postgres, block_start: i64, block_end: i64) -> anyhow::Result<Vec<ReceiptRow>> {
    tracing::debug!(start = %block_start, end = %block_end, "findind receipts");

    let result = sqlx::query_file!("src/bin/importer/sql/select_downloaded_receipts_in_range.sql", block_start, block_end)
        .fetch_all(&pg.connection_pool)
        .await;

    match result {
        Ok(rows) => {
            let mut parsed_rows: Vec<ReceiptRow> = Vec::with_capacity(rows.len());
            for row in rows {
                let parsed = ReceiptRow {
                    block_number: row.block_number,
                    payload: row.payload.try_into()?,
                };

                parsed_rows.push(parsed);
            }
            Ok(parsed_rows)
        }
        Err(e) => log_and_err!(reason = e, "failed to retrieve receipts"),
    }
}

async fn db_retrieve_max_imported_block(pg: &Postgres) -> anyhow::Result<Option<BlockNumber>> {
    tracing::debug!("finding max imported block");

    let result = sqlx::query_file_scalar!("src/bin/importer/sql/select_max_imported_block.sql")
        .fetch_one(&pg.connection_pool)
        .await;

    let block_number: i64 = match result {
        Ok(Some(max)) => max,
        Ok(None) => return Ok(None),
        Err(e) => return log_and_err!(reason = e, "failed to retrieve max block number"),
    };

    Ok(Some(block_number.into()))
}
