use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

#[derive(Debug, Clone)]
pub struct Postgres {
    pub sqlx_pool: PgPool,
}

impl Postgres {
    pub async fn new(url: &str) -> eyre::Result<Self> {
        tracing::info!("initing postgres");

        let sqlx_pool = PgPoolOptions::new()
            .min_connections(1)
            .max_connections(100)
            .acquire_timeout(Duration::from_secs(2))
            .connect(url)
            .await?;

        Ok(Self { sqlx_pool })
    }
}
