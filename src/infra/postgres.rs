//! PostgreSQL client.

use std::str::FromStr;
use std::time::Duration;

use anyhow::{anyhow, Context};
use ethereum_types::H160;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use sqlx::types::BigDecimal;

use crate::eth::miner::BlockMiner;
use crate::eth::primitives::{Wei, Address, Account, BlockNumber};
use crate::eth::storage::{EthStorage, test_accounts};

#[derive(Debug, Clone)]
pub struct Postgres {
    pub connection_pool: PgPool,
}

impl Postgres {
    pub async fn new(url: &str) -> anyhow::Result<Self> {
        tracing::info!(%url, "connecting to postgres");

        let connection_pool = PgPoolOptions::new()
            .min_connections(1)
            .max_connections(100)
            .acquire_timeout(Duration::from_secs(2))
            .connect(url)
            .await
            .map_err(|e| {
                tracing::error!(reason = ?e, %url, "failed to connect to postgres");
                anyhow!("Failed to connect to postgres")
            })?;

        let postgres = Self { connection_pool };
        let block = BlockMiner::genesis();
        postgres.save_block(block).await;

        postgres.insert_test_accounts_in_genesis(test_accounts()).await;

        Ok(postgres)
    }

    async fn insert_test_accounts_in_genesis(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        for acc in accounts {
            sqlx::query_file!(
                "src/eth/storage/postgres/queries/insert_account.sql",
                acc.address.as_bytes(),
                BigDecimal::from(acc.balance),
                BigDecimal::from(acc.nonce),
                acc.bytecode.as_deref(),
                i64::try_from(BlockNumber::ZERO)?
            )
            .execute(&self.connection_pool)
            .await
            .context("failed to insert account")?;
        }
        Ok(())
    }
}

