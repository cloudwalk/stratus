pub use self::inmemory::InMemoryPermanentStorage;
pub use self::redis::RedisPermanentStorage;
pub use self::rocks::RocksPermanentStorage;
pub use self::rocks::RocksStorageState;

mod inmemory;
mod redis;
pub mod rocks;

use std::str::FromStr;
use std::time::Duration;

use anyhow::anyhow;
use clap::Parser;
use display_json::DebugAsJson;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::PointInTime;
use crate::ext::parse_duration;
use crate::log_and_err;

/// Permanent (committed) storage operations.
pub trait PermanentStorage: Send + Sync + 'static {
    // -------------------------------------------------------------------------
    // Block number
    // -------------------------------------------------------------------------

    /// Sets the last mined block number.
    fn set_mined_block_number(&self, number: BlockNumber) -> anyhow::Result<()>;

    // Retrieves the last mined block number.
    fn read_mined_block_number(&self) -> anyhow::Result<BlockNumber>;

    // -------------------------------------------------------------------------
    // Block
    // -------------------------------------------------------------------------

    /// Persists atomically changes from block.
    fn save_block(&self, block: Block) -> anyhow::Result<()>;

    /// Persists atomically changes from blocks.
    fn save_block_batch(&self, blocks: Vec<Block>) -> anyhow::Result<()> {
        blocks.into_iter().try_for_each(|block| self.save_block(block))
    }

    /// Retrieves a block from the storage.
    fn read_block(&self, block_filter: BlockFilter) -> anyhow::Result<Option<Block>>;

    /// Retrieves a transaction from the storage.
    fn read_transaction(&self, hash: Hash) -> anyhow::Result<Option<TransactionMined>>;

    /// Retrieves logs from the storage.
    fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>>;

    // -------------------------------------------------------------------------
    // Account and slots
    // -------------------------------------------------------------------------

    /// Persists initial accounts (test accounts or genesis accounts).
    fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()>;

    /// Retrieves an account from the storage. Returns Option when not found.
    fn read_account(&self, address: Address, point_in_time: PointInTime) -> anyhow::Result<Option<Account>>;

    /// Retrieves an slot from the storage. Returns Option when not found.
    fn read_slot(&self, address: Address, index: SlotIndex, point_in_time: PointInTime) -> anyhow::Result<Option<Slot>>;

    // -------------------------------------------------------------------------
    // Global state
    // -------------------------------------------------------------------------

    #[cfg(feature = "dev")]
    /// Resets all state to a specific block number.
    fn reset(&self) -> anyhow::Result<()>;
}

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

/// Permanent storage configuration.
#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct PermanentStorageConfig {
    /// Permamenent storage implementation.
    #[arg(long = "perm-storage", env = "PERM_STORAGE")]
    pub perm_storage_kind: PermanentStorageKind,

    /// Storage connection URL.
    #[arg(long = "perm-storage-url", env = "PERM_STORAGE_URL", required_if_eq_any([("perm_storage_kind", "redis")]))]
    pub perm_storage_url: Option<String>,

    /// RocksDB storage path prefix to execute multiple local Stratus instances.
    #[arg(long = "rocks-path-prefix", env = "ROCKS_PATH_PREFIX")]
    pub rocks_path_prefix: Option<String>,

    /// The maximum time to wait for the RocksDB `wait_for_compaction` shutdown call.
    #[arg(long = "rocks-shutdown-timeout", env = "ROCKS_SHUTDOWN_TIMEOUT", value_parser=parse_duration, default_value = "4m")]
    pub rocks_shutdown_timeout: Duration,

    /// Augments or decreases the size of Column Family caches based on a multiplier.
    #[arg(long = "rocks-cache-size-multiplier", env = "ROCKS_CACHE_SIZE_MULTIPLIER")]
    pub rocks_cache_size_multiplier: Option<f32>,

    /// Augments or decreases the size of Column Family caches based on a multiplier.
    #[arg(long = "rocks-disable-sync-write", env = "ROCKS_DISABLE_SYNC_WRITE")]
    pub rocks_disable_sync_write: bool,
}

#[derive(DebugAsJson, Clone, serde::Serialize)]
pub enum PermanentStorageKind {
    #[serde(rename = "inmemory")]
    InMemory,

    #[serde(rename = "redis")]
    Redis,

    #[serde(rename = "rocks")]
    Rocks,
}

impl PermanentStorageConfig {
    /// Initializes permanent storage implementation.
    pub fn init(&self) -> anyhow::Result<Box<dyn PermanentStorage>> {
        tracing::info!(config = ?self, "creating permanent storage");

        let perm: Box<dyn PermanentStorage> = match self.perm_storage_kind {
            PermanentStorageKind::InMemory => Box::<InMemoryPermanentStorage>::default(),

            PermanentStorageKind::Redis => {
                let Some(url) = self.perm_storage_url.as_deref() else {
                    return log_and_err!("redis connection url not provided when it was expected to be present");
                };
                Box::new(RedisPermanentStorage::new(url)?)
            }

            PermanentStorageKind::Rocks => Box::new(RocksPermanentStorage::new(
                self.rocks_path_prefix.clone(),
                self.rocks_shutdown_timeout,
                self.rocks_cache_size_multiplier,
                !self.rocks_disable_sync_write,
            )?),
        };
        Ok(perm)
    }
}

impl FromStr for PermanentStorageKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        match s {
            "inmemory" => Ok(Self::InMemory),
            "redis" => Ok(Self::Redis),
            "rocks" => Ok(Self::Rocks),
            s => Err(anyhow!("unknown permanent storage: {}", s)),
        }
    }
}
