#![allow(dead_code)]

use serde_json::Value as JsonValue;
use sqlx::types::BigDecimal;
use sqlx::Row;
use stratus::eth::primitives::Address;
use stratus::eth::primitives::BlockNumber;
use stratus::eth::primitives::ExternalBlock;
use stratus::eth::primitives::ExternalReceipt;
use stratus::eth::primitives::Hash;
use stratus::eth::primitives::Wei;
use stratus::infra::postgres::Postgres;
use stratus::log_and_err;

// -----------------------------------------------------------------------------
// Queries
// -----------------------------------------------------------------------------

// Blocks

pub async fn pg_retrieve_max_downloaded_block(pg: &Postgres, start: BlockNumber, end: BlockNumber) -> anyhow::Result<Option<BlockNumber>> {
    tracing::debug!(%start, %end, "retrieving max downloaded block");

    let result = sqlx::query_file_scalar!("src/bin/importer/sql/select_max_downloaded_block_in_range.sql", start.as_i64(), end.as_i64())
        .fetch_one(&pg.connection_pool)
        .await;

    match result {
        Ok(Some(max)) => Ok(Some(max.into())),
        Ok(None) => Ok(None),
        Err(e) => log_and_err!(reason = e, "failed to retrieve max block number"),
    }
}

pub async fn pg_retrieve_max_imported_block(pg: &Postgres) -> anyhow::Result<Option<BlockNumber>> {
    tracing::debug!("retrieving max imported block");

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

pub async fn pg_init_blocks_cursor(pg: &Postgres) -> anyhow::Result<sqlx::Transaction<'_, sqlx::Postgres>> {
    let start = match pg_retrieve_max_imported_block(pg).await? {
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

pub async fn pg_fetch_blocks(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) -> anyhow::Result<Vec<BlockRow>> {
    tracing::debug!("fetching more blocks");

    let result = sqlx::query_file_scalar!("src/bin/importer/sql/cursor_fetch_downloaded_blocks.sql")
        .fetch_all(&mut **tx)
        .await;

    match result {
        Ok(rows) => {
            let mut parsed_rows: Vec<BlockRow> = Vec::with_capacity(rows.len());
            for row in rows {
                let parsed = BlockRow {
                    number: row.get_unchecked::<'_, i64, usize>(0).into(),
                    payload: row.get_unchecked::<'_, JsonValue, usize>(1).try_into()?,
                };
                parsed_rows.push(parsed);
            }
            Ok(parsed_rows)
        }
        Err(e) => log_and_err!(reason = e, "failed to fetch blocks from cursor"),
    }
}

// Receipts

pub async fn pg_retrieve_downloaded_receipts(pg: &Postgres, start: BlockNumber, end: BlockNumber) -> anyhow::Result<Vec<ReceiptRow>> {
    tracing::debug!(%start, %end, "retrieving receipts in range");

    let result = sqlx::query_file!("src/bin/importer/sql/select_downloaded_receipts_in_range.sql", start.as_i64(), end.as_i64())
        .fetch_all(&pg.connection_pool)
        .await;

    match result {
        Ok(rows) => {
            let mut parsed_rows: Vec<ReceiptRow> = Vec::with_capacity(rows.len());
            for row in rows {
                let parsed = ReceiptRow {
                    block_number: row.block_number.into(),
                    payload: row.payload.try_into()?,
                };

                parsed_rows.push(parsed);
            }
            Ok(parsed_rows)
        }
        Err(e) => log_and_err!(reason = e, "failed to retrieve receipts"),
    }
}

// Balances

pub async fn pg_retrieve_downloaded_balances(pg: &Postgres) -> anyhow::Result<Vec<BalanceRow>> {
    tracing::debug!("retrieving downloaded balances");

    let result = sqlx::query_file_as!(BalanceRow, "src/bin/importer/sql/select_downloaded_balances.sql")
        .fetch_all(&pg.connection_pool)
        .await;

    match result {
        Ok(accounts) => Ok(accounts),
        Err(e) => log_and_err!(reason = e, "failed to retrieve downloaded balances"),
    }
}

// -----------------------------------------------------------------------------
// Inserts
// -----------------------------------------------------------------------------
pub async fn pg_insert_balance(pg: &Postgres, address: Address, balance: Wei) -> anyhow::Result<()> {
    tracing::debug!(%address, %balance, "saving external balance");

    let result = sqlx::query_file!(
        "src/bin/importer/sql/insert_external_balance.sql",
        address.as_ref(),
        TryInto::<BigDecimal>::try_into(balance)?
    )
    .execute(&pg.connection_pool)
    .await;

    match result {
        Ok(_) => Ok(()),
        Err(e) => log_and_err!(reason = e, "failed to insert external balance"),
    }
}

pub async fn pg_insert_block_and_receipts(pg: &Postgres, number: BlockNumber, block: JsonValue, receipts: Vec<(Hash, JsonValue)>) -> anyhow::Result<()> {
    tracing::debug!(?block, ?receipts, "saving external block and receipts");

    let mut tx = pg.start_transaction().await?;

    // insert block
    let result = sqlx::query_file!("src/bin/importer/sql/insert_external_block.sql", number.as_i64(), block)
        .execute(&mut *tx)
        .await;

    match result {
        Ok(_) => {}
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            tracing::warn!(reason = ?e, "block unique violation, skipping");
        }
        Err(e) => return log_and_err!(reason = e, "failed to insert block"),
    }

    // insert receipts
    for (hash, receipt) in receipts {
        let result = sqlx::query_file!("src/bin/importer/sql/insert_external_receipt.sql", hash.as_ref(), number.as_i64(), receipt)
            .execute(&mut *tx)
            .await;

        match result {
            Ok(_) => {}
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                tracing::warn!(reason = ?e, "receipt unique violation, skipping");
            }
            Err(e) => return log_and_err!(reason = e, "failed to insert receipt"),
        }
    }

    pg.commit_transaction(tx).await?;

    Ok(())
}

// -----------------------------------------------------------------------------
// Types
// -----------------------------------------------------------------------------

pub struct BlockRow {
    pub number: BlockNumber,
    pub payload: ExternalBlock,
}

pub struct ReceiptRow {
    pub block_number: BlockNumber,
    pub payload: ExternalReceipt,
}

pub struct BalanceRow {
    pub address: Address,
    pub balance: Wei,
}
