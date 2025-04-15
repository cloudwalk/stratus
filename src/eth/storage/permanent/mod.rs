pub use self::rocks::RocksPermanentStorage;
pub use self::rocks::RocksStorageState;

pub mod rocks;

use std::time::Duration;

use clap::Parser;
use display_json::DebugAsJson;

/// Genesis file configuration
#[cfg(feature = "dev")]
use crate::config::GenesisFileConfig;
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

    /// Augments or decreases the size of Column Family caches based on a multiplier.
    #[arg(long = "rocks-cache-size-multiplier", env = "ROCKS_CACHE_SIZE_MULTIPLIER")]
    pub rocks_cache_size_multiplier: Option<f32>,

    /// Augments or decreases the size of Column Family caches based on a multiplier.
    #[arg(long = "rocks-disable-sync-write", env = "ROCKS_DISABLE_SYNC_WRITE")]
    pub rocks_disable_sync_write: bool,

    /// Interval for collecting RocksDB column family size metrics.
    #[arg(long = "rocks-cf-metrics-interval", env = "ROCKS_CF_METRICS_INTERVAL", value_parser=parse_duration)]
    pub rocks_cf_metrics_interval: Option<Duration>,

    /// Genesis file configuration
    #[clap(flatten)]
    #[cfg(feature = "dev")]
    pub genesis_file: GenesisFileConfig,
}

impl PermanentStorageConfig {
    /// Initializes permanent storage implementation.
    pub fn init(&self) -> anyhow::Result<RocksPermanentStorage> {
        tracing::info!(config = ?self, "creating permanent storage");

        RocksPermanentStorage::new(
            self.rocks_path_prefix.clone(),
            self.rocks_shutdown_timeout,
            self.rocks_cache_size_multiplier,
            !self.rocks_disable_sync_write,
            self.rocks_cf_metrics_interval,
        )
    }
}
