pub use self::rocks::RocksCfCacheConfig;
pub use self::rocks::RocksPermanentStorage;
pub use self::rocks::RocksStorageState;

pub mod rocks;

use std::time::Duration;

use clap::Parser;
use display_json::DebugAsJson;

/// Genesis file configuration
use crate::ext::parse_duration;

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

/// Permanent storage configuration.
#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct PermanentStorageConfig {
    /// RocksDB storage path prefix to execute multiple local Stratus instances.
    #[arg(long = "rocks-path-prefix", env = "ROCKS_PATH_PREFIX")]
    pub rocks_path_prefix: Option<String>,

    /// The maximum time to wait for the RocksDB `wait_for_compaction` shutdown call.
    #[arg(long = "rocks-shutdown-timeout", env = "ROCKS_SHUTDOWN_TIMEOUT", value_parser=parse_duration, default_value = "4m")]
    pub rocks_shutdown_timeout: Duration,

    /// Individual cache size configuration for each RocksDB Column Family.
    #[clap(flatten)]
    pub rocks_cf_cache: RocksCfCacheConfig,

    /// Disables sync write for RocksDB (improves performance but reduces durability).
    #[arg(long = "rocks-disable-sync-write", env = "ROCKS_DISABLE_SYNC_WRITE")]
    pub rocks_disable_sync_write: bool,

    /// Interval for collecting RocksDB column family size metrics.
    #[arg(long = "rocks-cf-size-metrics-interval", env = "ROCKS_CF_SIZE_METRICS_INTERVAL", value_parser=parse_duration)]
    pub rocks_cf_size_metrics_interval: Option<Duration>,

    /// Minimum number of file descriptors required for RocksDB initialization.
    #[arg(long = "rocks-file-descriptors-limit", env = "ROCKS_FILE_DESCRIPTORS_LIMIT", default_value = Self::DEFAULT_FILE_DESCRIPTORS_LIMIT)]
    pub rocks_file_descriptors_limit: u64,

    /// Path to the genesis.json file
    #[arg(long = "genesis-path", env = "GENESIS_JSON_PATH")]
    pub genesis_path: Option<String>,
}

impl PermanentStorageConfig {
    const DEFAULT_FILE_DESCRIPTORS_LIMIT: &'static str = if cfg!(feature = "dev") { "65536" } else { "1048576" };
    /// Initializes permanent storage implementation.
    pub fn init(&self) -> anyhow::Result<RocksPermanentStorage> {
        tracing::info!(config = ?self, "creating permanent storage");

        RocksPermanentStorage::new(
            self.rocks_path_prefix.clone(),
            self.rocks_shutdown_timeout,
            self.rocks_cf_cache.clone(),
            !self.rocks_disable_sync_write,
            self.rocks_cf_size_metrics_interval,
            self.rocks_file_descriptors_limit,
        )
    }
}
