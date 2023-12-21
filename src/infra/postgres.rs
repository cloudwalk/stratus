use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

#[derive(Debug, Clone)]
pub struct Postgres {
    pub connection_pool: PgPool,
    runtime_handle: tokio::runtime::Handle,
}

impl Postgres {
    pub async fn new(runtime_handle: tokio::runtime::Handle, url: &str) -> eyre::Result<Self> {
        tracing::info!("initing postgres");

        let connection_pool = PgPoolOptions::new()
            .min_connections(1)
            .max_connections(100)
            .acquire_timeout(Duration::from_secs(2))
            .connect(url)
            .await?;

        Ok(Self {
            connection_pool,
            runtime_handle,
        })
    }
}
