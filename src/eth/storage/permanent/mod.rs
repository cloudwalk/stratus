pub use self::rocks::RocksCfCacheConfig;
pub use self::rocks::RocksPermanentStorage;
pub use self::rocks::RocksStorageState;

pub mod rocks;

use std::time::Duration;

use clap::Parser;
use display_json::DebugAsJson;
use stratus_macros::CliOverrides;

/// Genesis file configuration
#[cfg(feature = "dev")]
use crate::config::GenesisFileConfig;
use crate::ext::duration_serde;
use crate::ext::option_duration_serde;
use crate::ext::parse_duration;

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

/// Permanent storage configuration.
#[derive(DebugAsJson, Clone, Parser, serde::Deserialize, serde::Serialize, CliOverrides)]
#[serde(default, deny_unknown_fields)]
pub struct PermanentStorageConfig {
    /// RocksDB storage path prefix to execute multiple local Stratus instances.
    #[arg(long = "rocks-path-prefix")]
    #[serde(rename = "path_prefix")]
    pub rocks_path_prefix: Option<String>,

    /// The maximum time to wait for the RocksDB `wait_for_compaction` shutdown call.
    #[arg(long = "rocks-shutdown-timeout", value_parser = parse_duration, default_value = "4m")]
    #[serde(rename = "shutdown_timeout", with = "duration_serde")]
    pub rocks_shutdown_timeout: Duration,

    /// Individual cache size configuration for each RocksDB Column Family.
    #[clap(flatten)]
    #[serde(rename = "cf_cache")]
    pub rocks_cf_cache: RocksCfCacheConfig,

    /// Disables sync write for RocksDB (improves performance but reduces durability).
    #[arg(long = "rocks-disable-sync-write")]
    #[serde(rename = "disable_sync_write")]
    pub rocks_disable_sync_write: bool,

    /// Interval for collecting RocksDB column family size metrics.
    #[arg(long = "rocks-cf-size-metrics-interval", value_parser = parse_duration)]
    #[serde(rename = "cf_size_metrics_interval", with = "option_duration_serde")]
    pub rocks_cf_size_metrics_interval: Option<Duration>,

    /// Minimum number of file descriptors required for RocksDB initialization.
    #[arg(long = "rocks-file-descriptors-limit", default_value = Self::DEFAULT_FILE_DESCRIPTORS_LIMIT)]
    #[serde(rename = "file_descriptors_limit")]
    pub rocks_file_descriptors_limit: u64,

    /// Genesis file configuration
    #[clap(flatten)]
    #[cfg(feature = "dev")]
    #[serde(rename = "genesis")]
    pub genesis_file: GenesisFileConfig,
}

impl Default for PermanentStorageConfig {
    fn default() -> Self {
        Self {
            rocks_path_prefix: None,
            rocks_shutdown_timeout: Duration::from_secs(4 * 60),
            rocks_cf_cache: RocksCfCacheConfig::default(),
            rocks_disable_sync_write: false,
            rocks_cf_size_metrics_interval: None,
            rocks_file_descriptors_limit: Self::default_file_descriptors_limit(),
            #[cfg(feature = "dev")]
            genesis_file: GenesisFileConfig::default(),
        }
    }
}

impl PermanentStorageConfig {
    const fn default_file_descriptors_limit() -> u64 {
        if cfg!(feature = "dev") { 65536 } else { 1048576 }
    }
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
