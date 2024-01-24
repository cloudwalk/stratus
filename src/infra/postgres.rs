//! PostgreSQL client.

use std::time::Duration;

use anyhow::anyhow;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

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

        Ok(Self { connection_pool })
    }
}
