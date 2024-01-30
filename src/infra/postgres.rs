//! PostgreSQL client.

use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::BigDecimal;
use sqlx::PgPool;

use crate::eth::miner::BlockMiner;
use crate::eth::primitives::Account;
use crate::eth::primitives::BlockNumber;
use crate::eth::storage::test_accounts;
use crate::eth::storage::EthStorage;

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
                anyhow!("failed to connect to postgres")
            })?;

        let postgres = Self { connection_pool };

        postgres.save_block(BlockMiner::genesis()).await?;
        postgres.insert_test_accounts_in_genesis(test_accounts()).await?;

        Ok(postgres)
    }

    async fn insert_test_accounts_in_genesis(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        tracing::debug!("adding test accounts to genesis block");

        for acc in accounts {
            sqlx::query_file!(
                "src/eth/storage/postgres/queries/insert_account.sql",
                acc.address.as_bytes(),
                BigDecimal::try_from(acc.nonce)?,
                BigDecimal::try_from(acc.balance)?,
                acc.bytecode.as_deref(),
                i64::try_from(BlockNumber::ZERO).context("failed to convert block number")?
            )
            .execute(&self.connection_pool)
            .await
            .context("failed to insert account")?;
        }

        Ok(())
    }
}
