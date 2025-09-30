#![allow(clippy::panic)]

use std::time::Duration;

use log::LevelFilter;
use sqlx::ConnectOptions;
use sqlx::PgPool;
use sqlx::postgres::PgConnectOptions;
use sqlx::postgres::PgPoolOptions;

use crate::eth::primitives::Address;
use crate::eth::primitives::Hash;
use crate::log_and_err;

pub struct PostgresBlockscout {
    pool: PgPool,
}

#[derive(Debug)]
pub struct PostgresBlockscoutConfig {
    pub url: String,
    pub connections: u32,
    pub acquire_timeout: Duration,
    pub slow_query_warn_threshold: Duration,
}

impl PostgresBlockscout {
    /// Creates a new [`PostgresBlockscout`].
    pub async fn new(config: PostgresBlockscoutConfig) -> anyhow::Result<Self> {
        tracing::info!(?config, "creating postgres blockscout connection");

        let options = config
            .url
            .as_str()
            .parse::<PgConnectOptions>()?
            .log_slow_statements(LevelFilter::Warn, config.slow_query_warn_threshold);

        let result = PgPoolOptions::new()
            .min_connections(config.connections)
            .max_connections(config.connections)
            .acquire_timeout(config.acquire_timeout)
            .connect_with(options)
            .await;

        let pool = match result {
            Ok(pool) => pool,
            Err(e) => return log_and_err!(reason = e, "failed to create postgres blockscout connection"),
        };

        Ok(Self { pool })
    }

    /// Fetch the 'from' address for a transaction by its hash from blockscout.
    pub async fn read_transaction_from(&self, tx_hash: Hash) -> anyhow::Result<Option<Address>> {
        tracing::debug!(%tx_hash, "retrieving transaction from address from blockscout");

        let result = sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT from_address_hash FROM transactions WHERE hash = $1 LIMIT 1"
        )
        .bind(tx_hash.as_ref() as &[u8])
        .fetch_optional(&self.pool)
        .await;

        match result {
            Ok(Some(from_address_hash)) => {
                let address: Address = from_address_hash.try_into()?;
                Ok(Some(address))
            }
            Ok(None) => {
                tracing::warn!(%tx_hash, "transaction not found in blockscout");
                Ok(None)
            }
            Err(e) => log_and_err!(reason = e, "failed to retrieve transaction from address from blockscout"),
        }
    }
}
