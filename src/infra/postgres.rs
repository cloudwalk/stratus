use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::eth::EthError;

#[derive(Debug, Clone)]
pub struct Postgres {
    pub connection_pool: PgPool,
}

impl Postgres {
    pub async fn new(url: &str) -> eyre::Result<Self> {
        tracing::info!("Connecting to PostgreSQL on {url}");

        let connection_pool = PgPoolOptions::new()
            .min_connections(1)
            .max_connections(100)
            .acquire_timeout(Duration::from_secs(2))
            .connect(url)
            .await
            .map_err(|e| {
                tracing::trace!(reason = ?e, url = %url, "Failed to connect to Postgres");
                EthError::StorageConnectionError
            })?;

        Ok(Self { connection_pool })
    }
}
