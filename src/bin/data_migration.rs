use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use rocksdb::BlockBasedOptions;
use rocksdb::Cache;
use rocksdb::IteratorMode;
use rocksdb::Options;
use rocksdb::ReadOptions;
use rocksdb::WriteBatch;
use rocksdb::DB;
use stratus::eth::storage::permanent::RocksStorageState;
use sugars::btmap;

const ESTIMATE_NUM_KEYS: &str = "rocksdb.estimate-num-keys";
const GIGABYTE: usize = 1024 * 1024 * 1024;
const MEGABYTE: usize = 1024 * 1024;
const KILOBYTE: usize = 1024;

const GIGABYTE_U64: u64 = 1024 * 1024 * 1024;
const MEGABYTE_U64: u64 = 1024 * 1024;

pub enum CacheSetting {
    /// Enabled cache with the given size in bytes
    Enabled(usize),
    Disabled,
}

#[derive(Debug, Clone, Copy)]
pub enum DbConfig {
    LargeSSTFiles,
    FastWriteSST,
    Default,
}

impl Default for DbConfig {
    fn default() -> Self {
        Self::Default
    }
}

impl DbConfig {
    pub fn to_options(self, cache_setting: CacheSetting) -> Options {
        let mut opts = Options::default();
        let mut block_based_options = BlockBasedOptions::default();

        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        opts.increase_parallelism(16);

        match self {
            DbConfig::LargeSSTFiles => {
                // Set the compaction style to Level Compaction
                opts.set_compaction_style(rocksdb::DBCompactionStyle::Level);

                // Configure the size of SST files at each level
                opts.set_target_file_size_base(512 * MEGABYTE_U64);

                // Increase the file size multiplier to expand file size at upper levels
                opts.set_target_file_size_multiplier(2); // Each level grows in file size quicker

                // Reduce the number of L0 files that trigger compaction, increasing frequency
                opts.set_level_zero_file_num_compaction_trigger(2);

                // Reduce thresholds for slowing and stopping writes, which forces more frequent compaction
                opts.set_level_zero_slowdown_writes_trigger(10);
                opts.set_level_zero_stop_writes_trigger(20);

                // Increase the max bytes for L1 to allow more data before triggering compaction
                opts.set_max_bytes_for_level_base(2 * GIGABYTE_U64);

                // Increase the level multiplier to aggressively increase space at each level
                opts.set_max_bytes_for_level_multiplier(8.0); // Exponential growth of levels is more pronounced

                // Configure block size to optimize for larger blocks, improving sequential read performance
                block_based_options.set_block_size(128 * KILOBYTE);

                // Increase the number of write buffers to delay flushing, optimizing CPU usage for compaction
                opts.set_max_write_buffer_number(5);
                opts.set_write_buffer_size(128 * MEGABYTE); // 128MB per write buffer

                // Keep a higher number of open files to accommodate more files being produced by aggressive compaction
                opts.set_max_open_files(20_000);

                // Apply more aggressive compression settings, if I/O and CPU permit
                opts.set_compression_per_level(&[
                    rocksdb::DBCompressionType::Lz4,
                    rocksdb::DBCompressionType::Zstd, // Use Zstd for higher compression from L1 onwards
                ]);
            }
            DbConfig::FastWriteSST => {
                // Continue using Level Compaction due to its effective use of I/O and CPU for writes
                opts.set_compaction_style(rocksdb::DBCompactionStyle::Level);

                // Increase initial SST file sizes to reduce the frequency of writes to disk
                opts.set_target_file_size_base(512 * MEGABYTE_U64); // Starting at 512MB for L1

                // Minimize the file size multiplier to control the growth of file sizes at upper levels
                opts.set_target_file_size_multiplier(1); // Minimal increase in file size at upper levels

                // Increase triggers for write slowdown and stop to maximize buffer before I/O actions
                opts.set_level_zero_file_num_compaction_trigger(100); // Slow down writes at 100 L0 files
                opts.set_level_zero_stop_writes_trigger(200); // Stop writes at 200 L0 files

                // Expand the maximum bytes for base level to further delay the need for compaction-related I/O
                opts.set_max_bytes_for_level_base(2048 * MEGABYTE_U64);

                // Use a higher level multiplier to increase space exponentially at higher levels
                opts.set_max_bytes_for_level_multiplier(10.0);

                // Opt for larger block sizes to decrease the number of read and write operations to disk
                block_based_options.set_block_size(512 * KILOBYTE); // 512KB blocks

                // Maximize the use of write buffers to extend the time data stays in memory before flushing
                opts.set_max_write_buffer_number(16);
                opts.set_write_buffer_size(GIGABYTE); // 1GB per write buffer

                // Allow a very high number of open files to minimize the overhead of opening and closing files
                opts.set_max_open_files(20_000);

                // Choose compression that balances CPU use and effective storage reduction
                opts.set_compression_per_level(&[rocksdb::DBCompressionType::Lz4, rocksdb::DBCompressionType::Zstd]);

                // Enable settings that make full use of CPU to handle more data in memory and process compaction
                opts.set_allow_concurrent_memtable_write(true);
                opts.set_enable_write_thread_adaptive_yield(true);
            }
            DbConfig::Default => {
                block_based_options.set_ribbon_filter(15.5); // https://github.com/facebook/rocksdb/wiki/RocksDB-Bloom-Filter

                opts.set_allow_concurrent_memtable_write(true);
                opts.set_enable_write_thread_adaptive_yield(true);

                let transform = rocksdb::SliceTransform::create_fixed_prefix(10);
                opts.set_prefix_extractor(transform);
                opts.set_memtable_prefix_bloom_ratio(0.2);

                // Enable a size-tiered compaction style, which is good for workloads with a high rate of updates and overwrites
                opts.set_compaction_style(rocksdb::DBCompactionStyle::Universal);

                let mut universal_compact_options = rocksdb::UniversalCompactOptions::default();
                universal_compact_options.set_size_ratio(10);
                universal_compact_options.set_min_merge_width(2);
                universal_compact_options.set_max_merge_width(6);
                universal_compact_options.set_max_size_amplification_percent(50);
                universal_compact_options.set_compression_size_percent(-1);
                universal_compact_options.set_stop_style(rocksdb::UniversalCompactionStopStyle::Total);
                opts.set_universal_compaction_options(&universal_compact_options);

                let pt_opts = rocksdb::PlainTableFactoryOptions {
                    user_key_length: 0,
                    bloom_bits_per_key: 10,
                    hash_table_ratio: 0.75,
                    index_sparseness: 8,
                    encoding_type: rocksdb::KeyEncodingType::Plain, // Default encoding
                    full_scan_mode: false,                          // Optimized for point lookups rather than full scans
                    huge_page_tlb_size: 0,                          // Not using huge pages
                    store_index_in_file: false,                     // Store index in memory for faster access
                };
                opts.set_plain_table_factory(&pt_opts);
            }
        }
        if let CacheSetting::Enabled(cache_size) = cache_setting {
            let cache = Cache::new_lru_cache(cache_size);
            block_based_options.set_block_cache(&cache);
            block_based_options.set_cache_index_and_filter_blocks(true);
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

fn generate_cf_options_map() -> BTreeMap<&'static str, Options> {
    btmap! {
        "accounts" => DbConfig::Default.to_options(CacheSetting::Disabled),
        "accounts_history" => DbConfig::FastWriteSST.to_options(CacheSetting::Disabled),
        "account_slots" => DbConfig::Default.to_options(CacheSetting::Disabled),
        "account_slots_history" => DbConfig::FastWriteSST.to_options(CacheSetting::Disabled),
        "transactions" => DbConfig::LargeSSTFiles.to_options(CacheSetting::Disabled),
        "blocks_by_number" => DbConfig::LargeSSTFiles.to_options(CacheSetting::Disabled),
        "blocks_by_hash" => DbConfig::LargeSSTFiles.to_options(CacheSetting::Disabled),
        "logs" => DbConfig::LargeSSTFiles.to_options(CacheSetting::Disabled),
    }
}

/// Open an existing database with the specified column families
fn open_db(path: &str, cf_options_map: &BTreeMap<&'static str, Options>) -> Result<Arc<DB>> {
    let db_path = Path::new(path);
    if !db_path.exists() {
        bail!("Database path does not exist: {}", path);
    }

    let cf_names = cf_options_map.keys().copied().collect::<Vec<_>>();
    let cf_options = cf_options_map.values().cloned().collect::<Vec<_>>();

    tracing::debug!("Opening database at {} with column families: {:?}", path, cf_names);

    let db = DB::open_cf_with_opts(&Options::default(), path, cf_names.iter().zip(cf_options)).context("Failed to open source database")?;

    Ok(Arc::new(db))
}

/// Count the number of entries in a column family using RocksDB's estimate
fn count_cf_entries(db: &Arc<DB>, cf_name: &str) -> Result<usize> {
    let cf = db.cf_handle(cf_name).context(format!("Failed to get column family handle for {}", cf_name))?;

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
        .context(format!("Failed to get source column family handle for {}", cf_name))?;

    let dest_cf = dest_db
        .cf_handle(cf_name)
        .context(format!("Failed to get destination column family handle for {}", cf_name))?;

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
    progress.set_message(format!("Migrating {}", cf_name));

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

    progress.finish_with_message(format!("Completed migrating {} ({} entries)", cf_name, total_migrated));

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
    let dest_db = RocksStorageState::new(args.destination, std::time::Duration::from_secs(240), Some(0.0), false)?
        .db
        .clone();

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
