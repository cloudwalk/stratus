use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use async_trait::async_trait;
use clap::Parser;
use display_json::DebugAsJson;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Wei;
use crate::eth::storage::PostgresExternalRpcStorage;
use crate::eth::storage::PostgresExternalRpcStorageConfig;
use crate::ext::parse_duration;
use crate::ext::JsonValue;

#[async_trait]
pub trait ExternalRpcStorage: Send + Sync {
    /// Read the largest block number saved inside a block range.
    async fn read_max_block_number_in_range(&self, start: BlockNumber, end: BlockNumber) -> anyhow::Result<Option<BlockNumber>>;

    /// Read all block inside a block range.
    async fn read_blocks_in_range(&self, start: BlockNumber, end: BlockNumber) -> anyhow::Result<Vec<ExternalBlock>>;

    /// Read all receipts inside a block range.
    async fn read_receipts_in_range(&self, start: BlockNumber, end: BlockNumber) -> anyhow::Result<Vec<ExternalReceipt>>;

    /// Read all initial accounts saved.
    async fn read_initial_accounts(&self) -> anyhow::Result<Vec<Account>>;

    /// Saves an initial account with its starting balance.
    async fn save_initial_account(&self, address: Address, balance: Wei) -> anyhow::Result<()>;

    /// Save an external block and its receipts to the storage.
    async fn save_block_and_receipts(&self, number: BlockNumber, block: JsonValue, receipts: Vec<(Hash, ExternalReceipt)>) -> anyhow::Result<()>;
}

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

/// External RPC storage configuration.
#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct ExternalRpcStorageConfig {
    /// External RPC storage implementation.
    #[arg(long = "external-rpc-storage", env = "EXTERNAL_RPC_STORAGE")]
    pub external_rpc_storage_kind: ExternalRpcStorageKind,

    /// External RPC storage number of parallel open connections.
    #[arg(long = "external-rpc-storage-connections", env = "EXTERNAL_RPC_STORAGE_CONNECTIONS")]
    pub external_rpc_storage_connections: u32,

    /// External RPC storage timeout when opening a connection.
    #[arg(long = "external-rpc-storage-timeout", value_parser=parse_duration, env = "EXTERNAL_RPC_STORAGE_TIMEOUT")]
    pub external_rpc_storage_timeout: Duration,
}

#[derive(DebugAsJson, Clone, serde::Serialize)]
pub enum ExternalRpcStorageKind {
    Postgres { url: String },
}

impl ExternalRpcStorageConfig {
    /// Initializes external rpc storage implementation.
    pub async fn init(&self) -> anyhow::Result<Arc<dyn ExternalRpcStorage>> {
        tracing::info!(config = ?self, "creating external rpc storage");

        match self.external_rpc_storage_kind {
            ExternalRpcStorageKind::Postgres { ref url } => {
                let config = PostgresExternalRpcStorageConfig {
                    url: url.to_owned(),
                    connections: self.external_rpc_storage_connections,
                    acquire_timeout: self.external_rpc_storage_timeout,
                };
                Ok(Arc::new(PostgresExternalRpcStorage::new(config).await?))
            }
        }
    }
}

impl FromStr for ExternalRpcStorageKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        match s {
            s if s.starts_with("postgres://") => Ok(Self::Postgres { url: s.to_string() }),
            s => Err(anyhow!("unknown external rpc storage: {}", s)),
        }
    }
}
