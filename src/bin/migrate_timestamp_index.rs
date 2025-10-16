/// Binary to populate the blocks_by_timestamp index for existing blocks.
/// 
/// This migration should be run once after deploying the timestamp index feature
/// to index all blocks that were saved before this feature was added.
/// 
/// Usage:
///   cargo run --bin migrate_timestamp_index -- --db-path ./data/rocksdb
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use stratus::eth::storage::permanent::RocksCfCacheConfig;
use stratus::eth::storage::permanent::RocksStorageState;

#[derive(Parser, Debug)]
#[clap(author, version, about = "Migrate existing blocks to add timestamp index")]
struct Args {
    /// Path to the RocksDB database
    #[clap(long, required = true)]
    db_path: String,

    /// Batch size for data migration (number of blocks per batch)
    #[clap(long, default_value = "10000")]
    batch_size: usize,
}

fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .init();

    // Parse command line arguments
    let args = Args::parse();

    tracing::info!("Opening RocksDB at: {}", args.db_path);
    tracing::info!("Batch size: {}", args.batch_size);

    // Open the database
    let state = RocksStorageState::new(
        args.db_path.clone(),
        std::time::Duration::from_secs(240),
        RocksCfCacheConfig::default_with_multiplier(0.1), // Use lower cache multiplier for migration
        false,
    )
    .context("Failed to open RocksDB")?;

    tracing::info!("Database opened successfully");

    // Run the migration
    let start_time = Instant::now();
    tracing::info!("Starting timestamp index migration...");

    let indexed_count = state
        .migrate_add_timestamp_index(args.batch_size)
        .context("Failed to migrate timestamp index")?;

    let duration = start_time.elapsed();

    tracing::info!(
        "Migration completed successfully in {:.2?}",
        duration
    );
    tracing::info!("Total blocks indexed: {}", indexed_count);

    // Print performance stats
    if indexed_count > 0 {
        let blocks_per_sec = indexed_count as f64 / duration.as_secs_f64();
        tracing::info!("Performance: {:.2} blocks/second", blocks_per_sec);
    }

    Ok(())
}

