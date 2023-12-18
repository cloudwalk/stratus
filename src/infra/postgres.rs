use sqlx::{postgres::PgPoolOptions, PgPool};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Postgres {
    pub sqlx_pool: PgPool,
}

impl Postgres {
    pub async fn new() -> eyre::Result<Self> {
        tracing::info!("initing postgres");

        let sqlx_pool = PgPoolOptions::new()
            .min_connections(1)
            .max_connections(100)
            .acquire_timeout(Duration::from_secs(2))
            .connect("postgres://localhost:5432")
            .await?;

        Ok(Self { sqlx_pool })
    }
}
