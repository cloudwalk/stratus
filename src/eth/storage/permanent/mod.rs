pub use self::rocks::RocksCfCacheConfig;
pub use self::rocks::RocksPermanentStorage;
pub use self::rocks::RocksStorageState;

pub mod rocks;

use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::bail;
use clap::Parser;
use display_json::DebugAsJson;

/// Genesis file configuration
#[cfg(feature = "dev")]
use crate::config::GenesisFileConfig;
use crate::ext::duration_serde;
use crate::ext::option_duration_serde;
use crate::ext::parse_duration;
use crate::globals::DATABASE_PATH;

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

/// Permanent storage configuration.
#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct PermanentStorageConfig {
    /// RocksDB storage path prefix to execute multiple local Stratus instances.
    #[arg(id = "storage.permanent.path_prefix", long = "rocks-path-prefix")]
    #[serde(rename = "path_prefix")]
    pub rocks_path_prefix: Option<String>,

    /// The maximum time to wait for the RocksDB `wait_for_compaction` shutdown call.
    #[arg(
        id = "storage.permanent.shutdown_timeout",
        long = "rocks-shutdown-timeout",
        value_parser = parse_duration,
        default_value = "4m"
    )]
    #[serde(rename = "shutdown_timeout", with = "duration_serde")]
    pub rocks_shutdown_timeout: Duration,

    /// Individual cache size configuration for each RocksDB Column Family.
    #[clap(flatten)]
    #[serde(rename = "cf_cache")]
    pub rocks_cf_cache: RocksCfCacheConfig,

    /// Disables sync write for RocksDB (improves performance but reduces durability).
    #[arg(id = "storage.permanent.disable_sync_write", long = "rocks-disable-sync-write")]
    #[serde(rename = "disable_sync_write")]
    pub rocks_disable_sync_write: bool,

    /// Interval for collecting RocksDB column family size metrics.
    #[arg(id = "storage.permanent.cf_size_metrics_interval", long = "rocks-cf-size-metrics-interval", value_parser = parse_duration)]
    #[serde(rename = "cf_size_metrics_interval", with = "option_duration_serde")]
    pub rocks_cf_size_metrics_interval: Option<Duration>,

    /// Minimum number of file descriptors required for RocksDB initialization.
    #[arg(
        id = "storage.permanent.file_descriptors_limit",
        long = "rocks-file-descriptors-limit",
        default_value = Self::DEFAULT_FILE_DESCRIPTORS_LIMIT
    )]
    #[serde(rename = "file_descriptors_limit")]
    pub rocks_file_descriptors_limit: u64,

    /// Genesis file configuration (available with the `dev` feature).
    #[clap(flatten)]
    #[cfg(feature = "dev")]
    #[serde(rename = "genesis")]
    pub genesis_file: GenesisFileConfig,

    /// Genesis file configuration is only available with the `dev` feature; the section is parsed
    /// and ignored on other builds so config files stay portable across binaries.
    #[arg(skip)]
    #[cfg(not(feature = "dev"))]
    #[serde(rename = "genesis", skip_serializing)]
    pub genesis_file: (),
}

#[cfg(test)]
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
            #[cfg(not(feature = "dev"))]
            genesis_file: (),
        }
    }
}

impl PermanentStorageConfig {
    #[cfg(test)]
    const fn default_file_descriptors_limit() -> u64 {
        if cfg!(feature = "dev") { 65536 } else { 1048576 }
    }
}

impl PermanentStorageConfig {
    const DEFAULT_FILE_DESCRIPTORS_LIMIT: &'static str = if cfg!(feature = "dev") { "65536" } else { "1048576" };

    /// Resolves the filesystem path of the node database.
    pub fn resolve_db_path(&self) -> anyhow::Result<String> {
        let Some(prefix) = &self.rocks_path_prefix else {
            tracing::info!("starting rocksdb storage - at default path: 'data/rocksdb'");
            return Ok("data/rocksdb".to_string());
        };

        if prefix.is_empty() {
            bail!("given prefix for RocksDB is empty, try not providing the flag");
        }

        if Path::new(prefix).is_dir() || Path::new(prefix).iter().count() > 1 {
            tracing::warn!(?prefix, "given prefix for RocksDB might put it in another folder");
        }

        let path = format!("{prefix}-rocksdb");
        tracing::info!("starting rocksdb storage - at custom path: '{:?}'", path);
        Ok(path)
    }

    /// Initializes permanent storage implementation.
    pub fn init(&self) -> anyhow::Result<RocksPermanentStorage> {
        tracing::info!(config = ?self, "creating permanent storage");

        let db_path = self.resolve_db_path()?;
        DATABASE_PATH
            .set(PathBuf::from(db_path.clone()))
            .map_err(|_| anyhow!("DATABASE_PATH is already set"))?;

        RocksPermanentStorage::new(
            db_path,
            self.rocks_shutdown_timeout,
            self.rocks_cf_cache.clone(),
            !self.rocks_disable_sync_write,
            self.rocks_cf_size_metrics_interval,
            self.rocks_file_descriptors_limit,
        )
    }
}
