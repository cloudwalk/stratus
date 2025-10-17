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
use rocksdb::{IteratorMode, ReadOptions, Direction, WriteBatch};
use stratus::eth::storage::permanent::rocks::rocks_db::create_or_open_db;
use stratus::eth::storage::permanent::rocks::rocks_state::generate_cf_options_map;
use stratus::eth::storage::permanent::RocksCfCacheConfig;
use stratus::eth::storage::permanent::rocks::cf_versions::{CfBlocksByNumberValue, CfBlocksByHashValue};
use stratus::eth::storage::permanent::rocks::types::{UnixTimeRocksdb, BlockNumberRocksdb};
use stratus::eth::storage::permanent::rocks::SerializeDeserializeWithContext as SerdeCtx;

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

    // Build CF options and open DB
    let cf_opts = generate_cf_options_map(&RocksCfCacheConfig::default_with_multiplier(0.1));
    let (db, _opts) = create_or_open_db(&args.db_path, &cf_opts).context("Failed to open RocksDB")?;
    tracing::info!("Database opened successfully");

    // Resolve CF handles
    let Some(cf_blocks_by_number) = db.cf_handle("blocks_by_number") else {
        anyhow::bail!("Missing 'blocks_by_number' column family");
    };
    let Some(cf_blocks_by_timestamp) = db.cf_handle("blocks_by_timestamp") else {
        anyhow::bail!("Missing 'blocks_by_timestamp' column family");
    };

    // Iterate and write in batches
    let start_time = Instant::now();
    tracing::info!("Starting timestamp index migration...");

    let mut batch = WriteBatch::default();
    let mut indexed_count: usize = 0;
    let mut batch_count: usize = 0;

    let mut ro = ReadOptions::default();
    ro.set_total_order_seek(true);
    let mut iter = db.iterator_cf_opt(&cf_blocks_by_number, ro, IteratorMode::From(&[], Direction::Forward));

    while let Some(Ok((key_bytes, value_bytes))) = iter.next() {
        // Decode components
        let block_number: BlockNumberRocksdb = SerdeCtx::deserialize_with_context(&key_bytes)
            .context("Failed to decode block number key")?;
        let block_cf_value: CfBlocksByNumberValue = SerdeCtx::deserialize_with_context(&value_bytes)
            .context("Failed to decode block value")?;
        let block = block_cf_value.into_inner();
        let ts = block.header.timestamp; // UnixTimeRocksdb-compatible value

        // Prepare insertion into blocks_by_timestamp: key=UnixTimeRocksdb, value=CfBlocksByHashValue(BlockNumberRocksdb)
        let ts_key: UnixTimeRocksdb = ts.into();
        let bn_val: CfBlocksByHashValue = block_number.into();

        let ts_key_bytes = SerdeCtx::serialize_with_context(&ts_key)?;
        let bn_val_bytes = SerdeCtx::serialize_with_context(&bn_val)?;
        batch.put_cf(&cf_blocks_by_timestamp, ts_key_bytes, bn_val_bytes);

        indexed_count += 1;
        batch_count += 1;
        if batch_count >= args.batch_size {
            tracing::info!("writing batch of {} blocks to timestamp index", batch_count);
            db.write(batch).context("Error writing batch to database")?;
            batch = WriteBatch::default();
            batch_count = 0;
        }
    }

    if batch_count > 0 {
        tracing::info!("writing final batch of {} blocks to timestamp index", batch_count);
        db.write(batch).context("Error writing final batch to database")?;
    }

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

