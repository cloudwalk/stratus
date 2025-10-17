/// Binary to peform data migrations when changing rocks config. For this to work copy the previous rocks config to this file.
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use clap::Parser;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use rocksdb::BlockBasedOptions;
use rocksdb::Cache;
use rocksdb::DB;
use rocksdb::DataBlockIndexType;
use rocksdb::IteratorMode;
use rocksdb::Options;
use rocksdb::ReadOptions;
use rocksdb::SliceTransform;
use rocksdb::WriteBatch;
use stratus::eth::storage::permanent::RocksCfCacheConfig;
use stratus::eth::storage::permanent::RocksStorageState;

const ESTIMATE_NUM_KEYS: &str = "rocksdb.estimate-num-keys";

// Copy of CacheSetting from the original rocks_config.rs for backward compatibility
pub enum CacheSetting {
    /// Enabled cache with the given size in bytes
    Enabled(usize),
    Disabled,
}

// Copy of DbConfig from the original rocks_config.rs for backward compatibility
#[derive(Debug, Clone, Copy)]
pub enum DbConfig {
    OptimizedPointLookUp,
    Default,
}

impl Default for DbConfig {
    fn default() -> Self {
        Self::Default
    }
}

impl DbConfig {
    pub fn to_options(self, cache_setting: CacheSetting, prefix_len: Option<usize>) -> Options {
        let mut opts = Options::default();
        let mut block_based_options = BlockBasedOptions::default();

        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        opts.increase_parallelism(16);

        block_based_options.set_pin_l0_filter_and_index_blocks_in_cache(true);
        block_based_options.set_cache_index_and_filter_blocks(true);
        block_based_options.set_bloom_filter(15.5, false);

        // due to the nature of our application enabling rocks metrics decreases point lookup performance by 5x.
        #[cfg(feature = "rocks_metrics")]
        {
            opts.enable_statistics();
            opts.set_statistics_level(rocksdb::statistics::StatsLevel::ExceptTimeForMutex);
        }

        if let Some(prefix_len) = prefix_len {
            let transform = SliceTransform::create_fixed_prefix(prefix_len);
            block_based_options.set_index_type(rocksdb::BlockBasedIndexType::HashSearch);
            opts.set_memtable_prefix_bloom_ratio(0.15);
            opts.set_prefix_extractor(transform);
        }

        if let CacheSetting::Enabled(cache_size) = cache_setting {
            let block_cache = Cache::new_lru_cache(cache_size / 2);
            let row_cache = Cache::new_lru_cache(cache_size / 2);

            opts.set_row_cache(&row_cache);
            block_based_options.set_block_cache(&block_cache);
        }

        match self {
            DbConfig::OptimizedPointLookUp => {
                block_based_options.set_data_block_hash_ratio(0.3);
                block_based_options.set_data_block_index_type(DataBlockIndexType::BinaryAndHash);

                opts.set_use_direct_reads(true);
                opts.set_memtable_whole_key_filtering(true);
                opts.set_compression_type(rocksdb::DBCompressionType::None);
            }
            DbConfig::Default => {
                opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
                opts.set_bottommost_compression_type(rocksdb::DBCompressionType::Zstd);
                opts.set_bottommost_compression_options(-14, 32767, 0, 16 * 1024, true); // mostly defaults except max_dict_bytes
                opts.set_bottommost_zstd_max_train_bytes(1600 * 1024, true);
            }
        }

        opts.set_block_based_table_factory(&block_based_options);

        opts
    }
}

#[derive(Parser, Debug)]
#[clap(author, version, about = "Migrate data from one RocksDB instance to another")]
struct Args {
    /// Path to the source RocksDB database
    #[clap(long, required = true)]
    source: String,

    /// Path to the destination RocksDB database
    #[clap(long, required = true)]
    destination: String,

    /// Batch size for data migration (number of key-value pairs per batch)
    #[clap(long, default_value = "10000")]
    batch_size: usize,
}

/// Generate CF options map for the source DB using our local copy of DbConfig
fn generate_cf_options_map() -> BTreeMap<&'static str, Options> {
    // BTreeMap is used to ensure the order of the column families creation is deterministic
    let mut cf_options = BTreeMap::new();

    // Define column families with their configurations
    cf_options.insert("accounts", DbConfig::OptimizedPointLookUp.to_options(CacheSetting::Disabled, None));
    cf_options.insert("accounts_history", DbConfig::Default.to_options(CacheSetting::Disabled, Some(20)));
    cf_options.insert("account_slots", DbConfig::OptimizedPointLookUp.to_options(CacheSetting::Disabled, Some(20)));
    cf_options.insert("account_slots_history", DbConfig::Default.to_options(CacheSetting::Disabled, Some(52)));
    cf_options.insert("transactions", DbConfig::Default.to_options(CacheSetting::Disabled, None));
    cf_options.insert("blocks_by_number", DbConfig::Default.to_options(CacheSetting::Disabled, None));
    cf_options.insert("blocks_by_hash", DbConfig::Default.to_options(CacheSetting::Disabled, None));
    cf_options.insert("blocks_by_timestamp", DbConfig::Default.to_options(CacheSetting::Disabled, None));
    cf_options.insert("block_changes", DbConfig::Default.to_options(CacheSetting::Disabled, None));

    cf_options
}

/// Open an existing database with the specified column families
fn open_db(path: &str, cf_options_map: &BTreeMap<&'static str, Options>) -> Result<Arc<DB>> {
    let db_path = Path::new(path);
    if !db_path.exists() {
        bail!("Database path does not exist: {path}");
    }

    let cf_names = cf_options_map.keys().copied().collect::<Vec<_>>();
    let cf_options = cf_options_map.values().cloned().collect::<Vec<_>>();

    tracing::debug!("Opening database at {} with column families: {:?}", path, cf_names);

    let db = DB::open_cf_with_opts(&Options::default(), path, cf_names.iter().zip(cf_options)).context("Failed to open source database")?;

    Ok(Arc::new(db))
}

/// Count the number of entries in a column family using RocksDB's estimate
fn count_cf_entries(db: &Arc<DB>, cf_name: &str) -> Result<usize> {
    let cf = db.cf_handle(cf_name).context(format!("Failed to get column family handle for {cf_name}"))?;

    // Get the estimated number of keys from RocksDB
    match db.property_value_cf(&cf, ESTIMATE_NUM_KEYS)? {
        Some(value) => {
            let count = value.parse::<usize>().context("Failed to parse estimate-num-keys property value")?;
            Ok(count)
        }
        None => Ok(0),
    }
}

/// Migrate data from source to destination for a specific column family
fn migrate_column_family(source_db: &Arc<DB>, dest_db: &Arc<DB>, cf_name: &str, batch_size: usize) -> Result<usize> {
    let source_cf = source_db
        .cf_handle(cf_name)
        .context(format!("Failed to get source column family handle for {cf_name}"))?;

    let dest_cf = dest_db
        .cf_handle(cf_name)
        .context(format!("Failed to get destination column family handle for {cf_name}"))?;

    // Count entries for progress reporting
    let total_entries = count_cf_entries(source_db, cf_name)?;

    // Create progress bar
    let progress = ProgressBar::new(total_entries as u64);
    progress.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("##-"),
    );
    progress.set_message(format!("Migrating {cf_name}"));

    let mut batch = WriteBatch::default();
    let mut total_migrated = 0;
    let mut batch_count = 0;

    // Iterate through all key-value pairs in the source column family
    let read_opts = ReadOptions::default();
    let iter = source_db.iterator_cf_opt(&source_cf, read_opts, IteratorMode::Start);

    for result in iter {
        let (key, value) = result.context("Error reading from source database")?;

        // Add key-value pair to the batch
        batch.put_cf(&dest_cf, key, value);
        batch_count += 1;
        total_migrated += 1;

        // Write batch when it reaches the specified size
        if batch_count >= batch_size {
            dest_db.write(batch).context("Error writing batch to destination database")?;
            batch = WriteBatch::default();
            batch_count = 0;
        }

        progress.inc(1);
    }

    // Write any remaining items in the batch
    if batch_count > 0 {
        dest_db.write(batch).context("Error writing final batch to destination database")?;
    }

    progress.finish_with_message(format!("Completed migrating {cf_name} ({total_migrated} entries)"));

    Ok(total_migrated)
}

fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Parse command line arguments
    let args = Args::parse();

    // Validate paths
    if args.source == args.destination {
        bail!("Source and destination paths must be different");
    }

    // Create destination directory if it doesn't exist
    if !Path::new(&args.destination).exists() {
        std::fs::create_dir_all(&args.destination).context(format!("Failed to create destination directory: {}", args.destination))?;
        tracing::info!("Created destination directory: {}", args.destination);
    }

    // Generate column family options for source (using our local copy of the config)
    let source_cf_options = generate_cf_options_map();

    // Open source database
    let source_db = open_db(&args.source, &source_cf_options)?;

    // Create destination database
    let state = RocksStorageState::new(
        args.destination,
        std::time::Duration::from_secs(240),
        RocksCfCacheConfig::default_with_multiplier(0.0),
        false,
    )?;
    let dest_db = Arc::clone(&state.db);

    // Get list of column families
    let cf_names = source_cf_options.keys().copied().collect::<Vec<_>>();

    // Migrate each column family
    let start_time = Instant::now();
    let mut migration_stats = HashMap::new();

    // Process each column family sequentially
    for cf_name in &cf_names {
        let result = migrate_column_family(&source_db, &dest_db, cf_name, args.batch_size)?;
        migration_stats.insert(*cf_name, result);
    }

    let migration_duration = start_time.elapsed();
    tracing::info!(
        "Migration completed in {:.2?}. Total entries migrated: {}",
        migration_duration,
        migration_stats.values().sum::<usize>()
    );

    tracing::info!("Data migration completed successfully");
    Ok(())
}
