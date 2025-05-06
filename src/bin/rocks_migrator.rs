//! RocksDB Migration binary.
//!
//! This tool opens a RocksDB database and copies its contents to another path
//! using the same configuration.

use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use indicatif::MultiProgress;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use rocksdb::IteratorMode;
use rocksdb::WriteBatch;
use stratus::eth::storage::permanent::rocks::RocksStorageState;
use stratus::utils::DropTimer;
#[cfg(all(not(target_env = "msvc"), any(feature = "jemalloc", feature = "jeprof")))]
use tikv_jemallocator::Jemalloc;
use tracing::Level;

#[cfg(all(not(target_env = "msvc"), any(feature = "jemalloc", feature = "jeprof")))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

/// Command line arguments for the RocksDB migration tool
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Source RocksDB path
    #[clap(long)]
    source: String,

    /// Destination RocksDB path
    #[clap(long)]
    destination: String,

    /// Cache size multiplier
    #[clap(long, default_value = "1.0")]
    cache_multiplier: f32,

    /// Shutdown timeout in seconds
    #[clap(long, default_value = "60")]
    shutdown_timeout: u64,

    /// Enable sync writes
    #[clap(long)]
    enable_sync_write: bool,

    /// Batch size for write operations
    #[clap(long, default_value = "1000")]
    batch_size: usize,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    let args = Args::parse();

    let _timer = DropTimer::start("rocks-migrator");

    run_migration(args)
}

fn run_migration(args: Args) -> Result<()> {
    let start_time = Instant::now();

    tracing::info!("Starting RocksDB migration");
    tracing::info!("Source: {}", args.source);
    tracing::info!("Destination: {}", args.destination);

    if Path::new(&args.destination).exists() {
        return Err(anyhow!("Destination path already exists: {}", args.destination));
    }

    tracing::info!("Opening source database");
    let source_state = open_rocks_db(
        &args.source,
        args.cache_multiplier,
        Duration::from_secs(args.shutdown_timeout),
        args.enable_sync_write,
    )?;

    tracing::info!("Creating destination database");
    let dest_state = open_rocks_db(
        &args.destination,
        args.cache_multiplier,
        Duration::from_secs(args.shutdown_timeout),
        args.enable_sync_write,
    )?;

    migrate_data(&source_state, &dest_state, args.batch_size)?;

    let elapsed = start_time.elapsed();
    tracing::info!("Migration completed in {:.2?}", elapsed);

    Ok(())
}

fn open_rocks_db(path: &str, cache_multiplier: f32, shutdown_timeout: Duration, enable_sync_write: bool) -> Result<RocksStorageState> {
    RocksStorageState::new(path.to_string(), shutdown_timeout, Some(cache_multiplier), enable_sync_write).context("Failed to open RocksDB")
}

fn migrate_data(source: &RocksStorageState, dest: &RocksStorageState, batch_size: usize) -> Result<()> {
    let column_families = vec![
        "accounts",
        "accounts_history",
        "account_slots",
        "account_slots_history",
        "transactions",
        "blocks_by_number",
        "blocks_by_hash",
    ];

    tracing::info!("Migrating {} column families: {:?}", column_families.len(), column_families);

    let mp = MultiProgress::new();
    let style = ProgressStyle::default_bar()
        .template("{msg} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
        .unwrap()
        .progress_chars("##-");

    for cf_name in column_families {
        let source_cf = source
            .db
            .cf_handle(cf_name)
            .ok_or_else(|| anyhow!("Column family not found in source: {}", cf_name))?;

        let dest_cf = dest
            .db
            .cf_handle(cf_name)
            .ok_or_else(|| anyhow!("Column family not found in destination: {}", cf_name))?;

        let key_count = source.db.property_int_value_cf(&source_cf, "rocksdb.estimate-num-keys")?.unwrap_or(1000); // Default to 1000 if we can't get the count

        tracing::info!(
            "Migrating column family '{}' with approximately {} keys (batch size: {})",
            cf_name,
            key_count,
            batch_size
        );

        let pb = mp.add(ProgressBar::new(key_count));
        pb.set_style(style.clone());
        pb.set_message(format!("Migrating '{}'", cf_name));

        let mut batch = WriteBatch::default();
        let mut batch_count = 0;
        let mut total_count = 0;

        let iter = source.db.iterator_cf(&source_cf, IteratorMode::Start);
        for result in iter {
            let (key, value) = result?;

            batch.put_cf(&dest_cf, &key, &value);
            batch_count += 1;
            total_count += 1;

            if batch_count >= batch_size {
                dest.db.write(batch)?;
                batch = WriteBatch::default();
                batch_count = 0;
                pb.set_position(total_count);
            }
        }

        if batch_count > 0 {
            dest.db.write(batch)?;
            pb.set_position(total_count);
        }

        pb.finish_with_message(format!("Completed '{}' ({} keys)", cf_name, total_count));
    }

    tracing::info!("All column families migrated successfully");
    Ok(())
}
