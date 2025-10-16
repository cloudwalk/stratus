# Timestamp Index Migration Guide

## Overview

This guide explains how to migrate existing blocks to add the timestamp index feature, which enables querying blocks by their timestamp using the new `stratus_getBlockByTimestamp` RPC method.

## Background

The timestamp index was added in issue [#2188](https://github.com/cloudwalk/stratus/issues/2188). This feature creates a new RocksDB column family that maps block timestamps to block numbers, allowing for efficient timestamp-based block lookups.

**Important**: Blocks saved before this feature was deployed will **not** be indexed by timestamp. This migration populates the index for all existing blocks.

## When to Run

Run this migration if:
- You have an existing Stratus node with historical blocks
- You want to query older blocks using `stratus_getBlockByTimestamp`
- The `blocks_by_timestamp` column family is empty or incomplete

**Note**: New blocks will be automatically indexed as they are saved, so this migration only needs to be run once.

## Prerequisites

1. Stop the Stratus node before running the migration (recommended but not required)
2. Ensure you have a recent backup of your database
3. Have sufficient disk space (the new index is relatively small, ~16 bytes per block)

## Running the Migration

### Basic Usage

```bash
cargo run --release --bin migrate_timestamp_index -- \
  --db-path ./data/rocksdb
```

### With Custom Batch Size

For better performance with large databases, you can adjust the batch size:

```bash
cargo run --release --bin migrate_timestamp_index -- \
  --db-path ./data/rocksdb \
  --batch-size 50000
```

### Command Line Options

- `--db-path`: Path to the RocksDB database directory (required)
- `--batch-size`: Number of blocks to process per write batch (default: 10000)
  - Larger batch sizes may improve performance but use more memory
  - Recommended values: 10000-100000

### Example Output

```
2025-01-16T10:30:00.000Z INFO Opening RocksDB at: ./data/rocksdb
2025-01-16T10:30:00.500Z INFO Database opened successfully
2025-01-16T10:30:00.501Z INFO starting migration to add timestamp index
2025-01-16T10:30:05.234Z INFO writing batch of 10000 blocks to timestamp index
2025-01-16T10:30:10.456Z INFO writing batch of 10000 blocks to timestamp index
...
2025-01-16T10:32:30.789Z INFO writing final batch of 5432 blocks to timestamp index
2025-01-16T10:32:30.890Z INFO migration completed: indexed 125432 blocks by timestamp
2025-01-16T10:32:30.890Z INFO Migration completed successfully in 2m 30.39s
2025-01-16T10:32:30.890Z INFO Total blocks indexed: 125432
2025-01-16T10:32:30.890Z INFO Performance: 830.21 blocks/second
```

## Performance Considerations

- **Estimated time**: ~1000-2000 blocks per second on typical hardware
- **Disk usage**: ~16 bytes per block (timestamp + block number)
- **Memory usage**: Depends on batch size (typically 50-500 MB)
- **Database operations**: Read from `blocks_by_number`, write to `blocks_by_timestamp`

## Verification

After the migration completes, verify it worked:

```bash
# Test querying a block by timestamp (example with timestamp 1700000000)
curl -X POST -H "Content-Type: application/json" --data '{
  "jsonrpc":"2.0",
  "method":"stratus_getBlockByTimestamp",
  "params":[1700000000, false],
  "id":1
}' http://localhost:3000
```

You should receive a block response if a block exists at or near that timestamp.

## Troubleshooting

### Migration fails with "Failed to open RocksDB"
- Ensure the database path is correct
- Check file permissions
- Verify the database is not corrupted

### Out of memory during migration
- Reduce the `--batch-size` parameter
- Close other applications using memory
- Increase system swap space if needed

### Migration is slow
- Increase the `--batch-size` parameter (try 50000 or 100000)
- Ensure the database is on fast storage (SSD recommended)
- Run with `--release` flag for optimized builds

### Timestamp queries still return null after migration
- Verify the migration completed successfully (check the log output)
- Ensure the correct database was migrated
- Check that the block you're querying actually exists

## Technical Details

### Database Structure

The migration creates entries in the `blocks_by_timestamp` column family:

```
Key:   UnixTimeRocksdb (u64 timestamp)
Value: CfBlocksByHashValue (wraps BlockNumberRocksdb)
```

### How It Works

1. Iterates through all blocks in `blocks_by_number` column family
2. Extracts the timestamp from each block header
3. Creates a mapping: `timestamp → block_number`
4. Writes entries in batches for efficiency
5. Reports progress and statistics

### Column Family Details

- **Name**: `blocks_by_timestamp`
- **Config**: `DbConfig::Default` with 2GB cache
- **Key ordering**: Lexicographic (timestamps in ascending order)
- **Compression**: LZ4 with Zstd for bottom-most level

## Integration with CI/CD

To automate this migration in your deployment pipeline:

```bash
#!/bin/bash
# deploy_with_migration.sh

# Stop the node
systemctl stop stratus

# Run migration
cargo run --release --bin migrate_timestamp_index -- \
  --db-path /var/lib/stratus/rocksdb \
  --batch-size 50000

# Start the node
systemctl start stratus
```

## Questions?

For issues or questions:
- Check the [original issue #2188](https://github.com/cloudwalk/stratus/issues/2188)
- Review the migration code in `src/bin/migrate_timestamp_index.rs`
- Contact the Stratus team

