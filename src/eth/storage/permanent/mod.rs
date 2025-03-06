pub use self::inmemory::InMemoryPermanentStorage;
pub use self::rocks::RocksPermanentStorage;
pub use self::rocks::RocksStorageState;

mod inmemory;
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
use crate::eth::primitives::PointInTime;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StorageError;
use crate::eth::primitives::TransactionMined;
use crate::ext::parse_duration;

/// Permanent (committed) storage operations.
pub trait PermanentStorage: Send + Sync + 'static {
    // -------------------------------------------------------------------------
    // Block number
    // -------------------------------------------------------------------------

    /// Sets the last mined block number.
    fn set_mined_block_number(&self, number: BlockNumber) -> anyhow::Result<(), StorageError>;

    // Retrieves the last mined block number.
    fn read_mined_block_number(&self) -> anyhow::Result<BlockNumber, StorageError>;

    // -------------------------------------------------------------------------
    // Block
    // -------------------------------------------------------------------------

    /// Persists atomically changes from block.
    fn save_block(&self, block: Block) -> anyhow::Result<(), StorageError>;

    /// Persists atomically changes from blocks.
    fn save_block_batch(&self, blocks: Vec<Block>) -> anyhow::Result<(), StorageError> {
        blocks.into_iter().try_for_each(|block| self.save_block(block))
    }

    /// Retrieves a block from the storage.
    fn read_block(&self, block_filter: BlockFilter) -> anyhow::Result<Option<Block>, StorageError>;

    /// Retrieves a transaction from the storage.
    fn read_transaction(&self, hash: Hash) -> anyhow::Result<Option<TransactionMined>, StorageError>;

    /// Retrieves logs from the storage.
    fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>, StorageError>;

    // -------------------------------------------------------------------------
    // Account and slots
    // -------------------------------------------------------------------------

    /// Persists initial accounts (test accounts or genesis accounts).
    fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<(), StorageError>;

    /// Retrieves an account from the storage. Returns Option when not found.
    fn read_account(&self, address: Address, point_in_time: PointInTime) -> anyhow::Result<Option<Account>, StorageError>;

    /// Retrieves an slot from the storage. Returns Option when not found.
    fn read_slot(&self, address: Address, index: SlotIndex, point_in_time: PointInTime) -> anyhow::Result<Option<Slot>, StorageError>;

    // -------------------------------------------------------------------------
    // Global state
    // -------------------------------------------------------------------------

    #[cfg(feature = "dev")]
    /// Resets all state to a specific block number.
    fn reset(&self) -> anyhow::Result<(), StorageError>;

    /// Returns the RocksDB latest sequence number for replication purposes.
    fn get_latest_sequence_number(&self) -> anyhow::Result<u64, StorageError>;

    /// Returns the WAL (Write-Ahead Log) updates that have occurred since the given sequence number.
    fn get_updates_since(&self, seq_number: u64) -> anyhow::Result<Vec<(u64, Vec<u8>)>, StorageError>;

    /// Applies a single WAL (Write-Ahead Log) update received from a leader node.
    fn apply_replication_log(&self, sequence: u64, log_data: Vec<u8>) -> anyhow::Result<BlockNumber, StorageError>;

    /// Creates a checkpoint of the RocksDB database at the specified path.
    fn create_checkpoint(&self, checkpoint_dir: &std::path::Path) -> anyhow::Result<(), StorageError>;

    /// Returns whether RocksDB replication is enabled
    fn rocksdb_replication_enabled(&self) -> bool;
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

    /// Use direct RocksDB replication instead of block re-execution for better performance
    #[arg(long = "use-rocksdb-replication", env = "USE_ROCKSDB_REPLICATION", default_value = "false")]
    pub use_rocksdb_replication: bool,

    /// Maximum number of replication logs to return in a single call to get_updates_since
    #[arg(long = "rocks-max-replication-logs", env = "ROCKS_MAX_REPLICATION_LOGS", default_value = "1")]
    pub rocks_max_replication_logs: usize,
}

#[derive(DebugAsJson, Clone, serde::Serialize)]
pub enum PermanentStorageKind {
    #[serde(rename = "inmemory")]
    InMemory,

    #[serde(rename = "rocks")]
    Rocks,
}

impl PermanentStorageConfig {
    /// Initializes permanent storage implementation.
    pub fn init(&self) -> anyhow::Result<Box<dyn PermanentStorage>> {
        tracing::info!(config = ?self, "creating permanent storage");

        let perm: Box<dyn PermanentStorage> = match self.perm_storage_kind {
            PermanentStorageKind::InMemory => Box::<InMemoryPermanentStorage>::default(),

            PermanentStorageKind::Rocks => Box::new(RocksPermanentStorage::new(
                self.rocks_path_prefix.clone(),
                self.rocks_shutdown_timeout,
                self.rocks_cache_size_multiplier,
                !self.rocks_disable_sync_write,
                self.use_rocksdb_replication,
                self.rocks_max_replication_logs,
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
            "rocks" => Ok(Self::Rocks),
            s => Err(anyhow!("unknown permanent storage: {}", s)),
        }
    }
}
