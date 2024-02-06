//! PostgreSQL client.

use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::BigDecimal;
use sqlx::PgPool;

use crate::eth::miner::BlockMiner;
use crate::eth::primitives::Account;
use crate::eth::storage::test_accounts;
use crate::eth::storage::EthStorage;

#[derive(Debug, Clone)]
pub struct Postgres {
    pub connection_pool: PgPool,
}

impl Postgres {
    pub async fn new(url: &str) -> anyhow::Result<Self> {
        tracing::info!(%url, "starting postgres client");

        let connection_pool = PgPoolOptions::new()
            .min_connections(1)
            .max_connections(100)
            .acquire_timeout(Duration::from_secs(2))
            .connect(url)
            .await
            .map_err(|e| {
                tracing::error!(reason = ?e, %url, "failed to start postgres client");
                anyhow!("failed to start postgres client")
            })?;

        let postgres = Self { connection_pool };

        postgres.insert_genesis().await?;

        Ok(postgres)
    }

    async fn insert_genesis(&self) -> anyhow::Result<()> {
        let genesis = sqlx::query!("SELECT number FROM blocks WHERE number = 0")
            .fetch_optional(&self.connection_pool)
            .await?;

        if genesis.is_none() {
            self.save_block(BlockMiner::genesis()).await?;
            self.insert_test_accounts_in_genesis(test_accounts()).await?;
        }

        Ok(())
    }

    async fn insert_test_accounts_in_genesis(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        tracing::debug!("adding test accounts to genesis block");

        for acc in accounts {
            let mut tx = self.connection_pool.begin().await.context("failed to init transaction")?;
            let block_number = 0;
            let balance = BigDecimal::try_from(acc.balance)?;
            let nonce = BigDecimal::try_from(acc.nonce)?;
            let bytecode = acc.bytecode.as_deref();

            sqlx::query_file!(
                "src/eth/storage/postgres/queries/insert_account.sql",
                acc.address.as_ref(),
                nonce,
                balance,
                bytecode,
                block_number
            )
            .execute(&mut *tx)
            .await
            .context("failed to insert account")?;

            sqlx::query!(
                "INSERT INTO historical_balances (address, balance, block_number) VALUES ($1, $2, $3)",
                acc.address.as_ref(),
                balance,
                block_number
            )
            .execute(&mut *tx)
            .await
            .context("failed to insert balance")?;

            sqlx::query!(
                "INSERT INTO historical_nonces (address, nonce, block_number) VALUES ($1, $2, $3)",
                acc.address.as_ref(),
                nonce,
                block_number
            )
            .execute(&mut *tx)
            .await
            .context("failed to insert nonce")?;

            tx.commit().await.context("Failed to commit transaction")?;
        }

        Ok(())
    }
}
