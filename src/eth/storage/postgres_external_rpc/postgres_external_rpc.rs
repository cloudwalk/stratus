use std::time::Duration;

use async_trait::async_trait;
use itertools::Itertools;
use serde_json::Value as JsonValue;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::BigDecimal;
use sqlx::PgPool;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Wei;
use crate::eth::storage::ExternalRpcStorage;
use crate::ext::traced_sleep;
use crate::ext::ResultExt;
use crate::ext::SleepReason;
use crate::log_and_err;

const MAX_RETRIES: u64 = 50;

pub struct PostgresExternalRpcStorage {
    pool: PgPool,
}

#[derive(Debug)]
pub struct PostgresExternalRpcStorageConfig {
    pub url: String,
    pub connections: u32,
    pub acquire_timeout: Duration,
}

impl PostgresExternalRpcStorage {
    /// Creates a new [`PostgresExternalRpcStorage`].
    pub async fn new(config: PostgresExternalRpcStorageConfig) -> anyhow::Result<Self> {
        tracing::info!(?config, "creating postgres external rpc storage");

        let result = PgPoolOptions::new()
            .min_connections(config.connections)
            .max_connections(config.connections)
            .acquire_timeout(config.acquire_timeout)
            .connect(&config.url)
            .await;

        let pool = match result {
            Ok(pool) => pool,
            Err(e) => return log_and_err!(reason = e, "failed to create postgres external rpc storage"),
        };

        Ok(Self { pool })
    }
}

#[async_trait]
impl ExternalRpcStorage for PostgresExternalRpcStorage {
    async fn read_max_block_number_in_range(&self, start: BlockNumber, end: BlockNumber) -> anyhow::Result<Option<BlockNumber>> {
        tracing::debug!(%start, %end, "retrieving max external block");

        let result = sqlx::query_file_scalar!(
            "src/eth/storage/postgres_external_rpc/sql/select_max_external_block_in_range.sql",
            start.as_i64(),
            end.as_i64()
        )
        .fetch_one(&self.pool)
        .await;

        match result {
            Ok(Some(max)) => Ok(Some(max.into())),
            Ok(None) => Ok(None),
            Err(e) => log_and_err!(reason = e, "failed to retrieve max block number"),
        }
    }

    async fn read_blocks_in_range(&self, start: BlockNumber, end: BlockNumber) -> anyhow::Result<Vec<ExternalBlock>> {
        tracing::debug!(%start, %end, "retrieving external blocks in range");
        let mut attempt: u64 = 1;

        loop {
            let result = sqlx::query_file!(
                "src/eth/storage/postgres_external_rpc/sql/select_external_blocks_in_range.sql",
                start.as_i64(),
                end.as_i64()
            )
            .fetch_all(&self.pool)
            .await;

            match result {
                Ok(rows) => {
                    let mut blocks: Vec<ExternalBlock> = Vec::with_capacity(rows.len());
                    for row in rows {
                        blocks.push(row.payload.try_into()?);
                    }
                    let blocks_sorted = blocks.into_iter().sorted_by_key(|x| x.number()).collect();
                    return Ok(blocks_sorted);
                }
                Err(e) =>
                    if attempt <= MAX_RETRIES {
                        tracing::warn!(reason = ?e, %attempt, "attempt failed. retrying now.");
                        attempt += 1;

                        let backoff = Duration::from_millis(attempt.pow(2));
                        traced_sleep(backoff, SleepReason::RetryBackoff).await;
                    } else {
                        return log_and_err!(reason = e, "failed to retrieve external blocks");
                    },
            }
        }
    }

    async fn read_receipts_in_range(&self, start: BlockNumber, end: BlockNumber) -> anyhow::Result<Vec<ExternalReceipt>> {
        tracing::debug!(%start, %end, "retrieving external receipts in range");
        let mut attempt: u64 = 1;

        loop {
            let result = sqlx::query_file!(
                "src/eth/storage/postgres_external_rpc/sql/select_external_receipts_in_range.sql",
                start.as_i64(),
                end.as_i64()
            )
            .fetch_all(&self.pool)
            .await;

            match result {
                Ok(rows) => {
                    let mut receipts: Vec<ExternalReceipt> = Vec::with_capacity(rows.len());
                    for row in rows {
                        receipts.push(row.payload.try_into()?);
                    }
                    return Ok(receipts);
                }
                Err(e) =>
                    if attempt <= MAX_RETRIES {
                        tracing::warn!(reason = ?e, %attempt, "attempt failed. retrying now.");
                        attempt += 1;

                        let backoff = Duration::from_millis(attempt.pow(2));
                        traced_sleep(backoff, SleepReason::RetryBackoff).await;
                    } else {
                        return log_and_err!(reason = e, "failed to retrieve receipts");
                    },
            }
        }
    }

    async fn read_initial_accounts(&self) -> anyhow::Result<Vec<Account>> {
        tracing::debug!("retrieving external balances");

        let result = sqlx::query_file!("src/eth/storage/postgres_external_rpc/sql/select_external_balances.sql")
            .fetch_all(&self.pool)
            .await;

        match result {
            Ok(rows) => {
                let mut accounts: Vec<Account> = Vec::with_capacity(rows.len());
                for row in rows {
                    let account = Account::new_with_balance(row.address.try_into()?, row.balance.try_into()?);
                    accounts.push(account);
                }
                Ok(accounts)
            }
            Err(e) => log_and_err!(reason = e, "failed to retrieve accounts with initial balances balances"),
        }
    }

    async fn save_initial_account(&self, address: Address, balance: Wei) -> anyhow::Result<()> {
        tracing::debug!(%address, %balance, "saving external balance");

        let result = sqlx::query_file!(
            "src/eth/storage/postgres_external_rpc/sql/insert_external_balance.sql",
            address.as_ref(),
            TryInto::<BigDecimal>::try_into(balance)?
        )
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) => log_and_err!(reason = e, "failed to insert external balance"),
        }
    }

    async fn save_block_and_receipts(&self, number: BlockNumber, block: JsonValue, receipts: Vec<(Hash, ExternalReceipt)>) -> anyhow::Result<()> {
        tracing::debug!(?block, ?receipts, "saving external block and receipts");

        let mut tx = match self.pool.begin().await {
            Ok(tx) => tx,
            Err(e) => return log_and_err!(reason = e, "failed to init postgres transaction"),
        };

        // insert block
        let result = sqlx::query_file!("src/eth/storage/postgres_external_rpc/sql/insert_external_block.sql", number.as_i64(), block)
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
            let receipt_json = serde_json::to_value(&receipt).expect_infallible();
            let result = sqlx::query_file!(
                "src/eth/storage/postgres_external_rpc/sql/insert_external_receipt.sql",
                hash.as_ref(),
                number.as_i64(),
                receipt_json
            )
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

        match tx.commit().await {
            Ok(_) => Ok(()),
            Err(e) => log_and_err!(reason = e, "failed to commit postgres transaction"),
        }
    }
}
