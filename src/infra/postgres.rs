//! PostgreSQL client.

use std::collections::HashMap;
use std::sync::Arc;

use std::time::Duration;

use anyhow::anyhow;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tokio::sync::RwLock;

use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;
use crate::log_and_err;

#[derive(Debug, Clone)]
pub struct Postgres {
    pub connection_pool: PgPool,
    pub sload_cache: Arc<RwLock<HashMap<(Address, SlotIndex), (SlotValue, BlockNumber)>>>,
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

        let postgres = Self {
            connection_pool,
            sload_cache: Arc::new(RwLock::new(HashMap::new())),
        };

        Ok(postgres)
    }

    /// Starts a new database transaction.
    pub async fn start_transaction(&self) -> anyhow::Result<sqlx::Transaction<'_, sqlx::Postgres>> {
        tracing::debug!("starting postgres transaction");

        match self.connection_pool.begin().await {
            Ok(tx) => Ok(tx),
            Err(e) => log_and_err!(reason = e, "failed to start postgres transaction"),
        }
    }

    /// Commits an running database transaction.
    pub async fn commit_transaction(&self, tx: sqlx::Transaction<'_, sqlx::Postgres>) -> anyhow::Result<()> {
        match tx.commit().await {
            Ok(_) => Ok(()),
            Err(e) => log_and_err!(reason = e, "failed to commit postgres transaction"),
        }
    }
}
