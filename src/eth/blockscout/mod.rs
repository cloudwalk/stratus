pub use postgres::PostgresBlockscout;
pub use postgres::PostgresBlockscoutConfig;

mod postgres;

use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use display_json::DebugAsJson;

use crate::eth::primitives::Address;
use crate::eth::primitives::Hash;
use crate::ext::parse_duration;

#[allow(async_fn_in_trait)]
pub trait Blockscout: Send + Sync {
    /// Read the 'from' address for a transaction by its hash.
    async fn read_transaction_from(&self, tx_hash: Hash) -> anyhow::Result<Option<Address>>;
}

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

/// Blockscout storage configuration.
#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct BlockscoutConfig {
    /// Blockscout storage URL (postgres://...).
    #[arg(long = "blockscout-storage", env = "BLOCKSCOUT_STORAGE")]
    pub blockscout_storage_url: Option<String>,

    /// Blockscout storage number of parallel open connections.
    #[arg(long = "blockscout-storage-connections", env = "BLOCKSCOUT_STORAGE_CONNECTIONS", default_value = "2")]
    pub blockscout_storage_connections: u32,

    /// Blockscout storage timeout when opening a connection.
    #[arg(long = "blockscout-storage-timeout", value_parser=parse_duration, env = "BLOCKSCOUT_STORAGE_TIMEOUT", default_value = "5s")]
    pub blockscout_storage_timeout: Duration,

    /// Blockscout threshold in seconds for warning slow queries.
    #[arg(long = "blockscout-slow-query-warn-threshold", value_parser=parse_duration, env = "BLOCKSCOUT_SLOW_QUERY_WARN_THRESHOLD", default_value = "1s")]
    pub blockscout_slow_query_warn_threshold: Duration,
}

impl BlockscoutConfig {
    /// Initializes blockscout storage implementation if configured.
    pub async fn init(&self) -> anyhow::Result<Option<Arc<PostgresBlockscout>>> {
        let Some(url) = &self.blockscout_storage_url else {
            return Ok(None);
        };

        tracing::info!(config = ?self, "creating blockscout storage");

        let config = PostgresBlockscoutConfig {
            url: url.to_owned(),
            connections: self.blockscout_storage_connections,
            acquire_timeout: self.blockscout_storage_timeout,
            slow_query_warn_threshold: self.blockscout_slow_query_warn_threshold,
        };

        Ok(Some(Arc::new(PostgresBlockscout::new(config).await?)))
    }
}

impl Blockscout for PostgresBlockscout {
    async fn read_transaction_from(&self, tx_hash: Hash) -> anyhow::Result<Option<Address>> {
        self.read_transaction_from(tx_hash).await
    }
}
