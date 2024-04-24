use std::marker::PhantomData;

use anyhow::anyhow;
use anyhow::Result;
use rocksdb::backup::BackupEngine;
use rocksdb::backup::BackupEngineOptions;
use rocksdb::backup::RestoreOptions;
use rocksdb::BlockBasedOptions;
use rocksdb::DBIteratorWithThreadMode;
use rocksdb::Env;
use rocksdb::IteratorMode;
use rocksdb::Options;
use rocksdb::WriteBatch;
use rocksdb::DB;
use serde::Deserialize;
use serde::Serialize;

pub enum DbConfig {
    LargeSSTFiles,
    FastWriteSST,
    Default,
}

// A generic struct that abstracts over key-value pairs stored in RocksDB.
pub struct RocksDb<K, V> {
    pub db: DB,
    _marker: PhantomData<(K, V)>,
}

impl<K: Serialize + for<'de> Deserialize<'de> + std::hash::Hash + Eq, V: Serialize + for<'de> Deserialize<'de> + Clone> RocksDb<K, V> {
    pub fn new(db_path: &str, config: DbConfig) -> anyhow::Result<Self> {
        let mut opts = Options::default();
        let mut block_based_options = BlockBasedOptions::default();

        opts.create_if_missing(true);
        opts.increase_parallelism(16);

        match config {
            DbConfig::LargeSSTFiles => {
                // Set the compaction style to Level Compaction
                opts.set_compaction_style(rocksdb::DBCompactionStyle::Level);

                // Configure the size of SST files at each level
                opts.set_target_file_size_base(512 * 1024 * 1024);

                // Increase the file size multiplier to expand file size at upper levels
                opts.set_target_file_size_multiplier(2); // Each level grows in file size quicker

                // Reduce the number of L0 files that trigger compaction, increasing frequency
                opts.set_level_zero_file_num_compaction_trigger(2);

                // Reduce thresholds for slowing and stopping writes, which forces more frequent compaction
                opts.set_level_zero_slowdown_writes_trigger(10);
                opts.set_level_zero_stop_writes_trigger(20);

                // Increase the max bytes for L1 to allow more data before triggering compaction
                opts.set_max_bytes_for_level_base(2048 * 1024 * 1024);

                // Increase the level multiplier to aggressively increase space at each level
                opts.set_max_bytes_for_level_multiplier(8.0); // Exponential growth of levels is more pronounced

                // Configure block size to optimize for larger blocks, improving sequential read performance
                block_based_options.set_block_size(128 * 1024); // 128KB blocks

                // Increase the number of write buffers to delay flushing, optimizing CPU usage for compaction
                opts.set_max_write_buffer_number(5);
                opts.set_write_buffer_size(128 * 1024 * 1024); // 128MB per write buffer

                // Keep a higher number of open files to accommodate more files being produced by aggressive compaction
                opts.set_max_open_files(20000);

                // Apply more aggressive compression settings, if I/O and CPU permit
                opts.set_compression_per_level(&[
                    rocksdb::DBCompressionType::None, // No compression for L0
                    rocksdb::DBCompressionType::Zstd, // Use Zstd for higher compression from L1 onwards
                ]);
            }
            DbConfig::FastWriteSST => {
                // Continue using Level Compaction due to its effective use of I/O and CPU for writes
                opts.set_compaction_style(rocksdb::DBCompactionStyle::Level);

                // Increase initial SST file sizes to reduce the frequency of writes to disk
                opts.set_target_file_size_base(512 * 1024 * 1024); // Starting at 512MB for L1

                // Minimize the file size multiplier to control the growth of file sizes at upper levels
                opts.set_target_file_size_multiplier(1); // Minimal increase in file size at upper levels

                // Increase triggers for write slowdown and stop to maximize buffer before I/O actions
                opts.set_level_zero_file_num_compaction_trigger(100);  // Slow down writes at 100 L0 files
                opts.set_level_zero_stop_writes_trigger(200);          // Stop writes at 200 L0 files

                // Expand the maximum bytes for base level to further delay the need for compaction-related I/O
                opts.set_max_bytes_for_level_base(2048 * 1024 * 1024); // 4GB at L1

                // Use a higher level multiplier to increase space exponentially at higher levels
                opts.set_max_bytes_for_level_multiplier(10);

                // Opt for larger block sizes to decrease the number of read and write operations to disk
                block_based_options.set_block_size(512 * 1024); // 512KB blocks

                // Maximize the use of write buffers to extend the time data stays in memory before flushing
                opts.set_max_write_buffer_number(16);
                opts.set_write_buffer_size(1024 * 1024 * 1024); // 1GB per write buffer

                // Allow a very high number of open files to minimize the overhead of opening and closing files
                opts.set_max_open_files(20000);

                // Choose compression that balances CPU use and effective storage reduction
                opts.set_compression_per_level(&[
                    rocksdb::DBCompressionType::LZ4,   // Fast compression with good performance and decent compression ratio
                ]);

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
        opts.set_block_based_table_factory(&block_based_options);

        let db = DB::open(&opts, db_path)?;

        Ok(RocksDb { db, _marker: PhantomData })
    }

    pub fn backup_path(&self) -> anyhow::Result<String> {
        Ok(format!("{}backup", self.db.path().to_str().ok_or(anyhow!("Invalid path"))?))
    }

    fn backup_engine(&self) -> anyhow::Result<BackupEngine> {
        let backup_opts = BackupEngineOptions::new(self.backup_path()?)?;
        let backup_env = Env::new()?;
        Ok(BackupEngine::open(&backup_opts, &backup_env)?)
    }

    pub fn backup(&self) -> anyhow::Result<()> {
        let mut backup_engine = self.backup_engine()?;
        backup_engine.create_new_backup(&self.db)?;
        backup_engine.purge_old_backups(2)?;
        Ok(())
    }

    pub fn restore(&self) -> anyhow::Result<()> {
        let mut backup_engine = self.backup_engine()?;
        let restore_options = RestoreOptions::default();
        backup_engine.restore_from_latest_backup(self.db.path(), self.backup_path()?, &restore_options)?;
        Ok(())
    }

    // Clears the database
    pub fn clear(&self) -> Result<()> {
        let mut batch = WriteBatch::default();
        for item in self.db.iterator(IteratorMode::Start) {
            let (key, _) = item?; // Handle or unwrap the Result
            batch.delete(key);
        }
        self.db.write(batch)?;
        Ok(())
    }

    pub fn get(&self, key: &K) -> Option<V> {
        let Ok(serialized_key) = bincode::serialize(key) else { return None };
        let Ok(Some(value_bytes)) = self.db.get(serialized_key) else { return None };

        bincode::deserialize(&value_bytes).ok()
    }

    pub fn get_current_block_number(&self) -> u64 {
        let Ok(serialized_key) = bincode::serialize(&"current_block") else {
            return 0;
        };
        let Ok(Some(value_bytes)) = self.db.get(serialized_key) else { return 0 };

        bincode::deserialize(&value_bytes).ok().unwrap_or(0)
    }

    pub fn get_index_block_number(&self) -> u64 {
        self.last_index().map(|(block_number, _)| block_number).unwrap_or(0)
    }

    // Mimics the 'insert' functionality of a HashMap
    pub fn insert(&self, key: K, value: V) {
        let serialized_key = bincode::serialize(&key).unwrap();
        let serialized_value = bincode::serialize(&value).unwrap();
        self.db.put(serialized_key, serialized_value).unwrap();
    }

    pub fn insert_batch(&self, changes: Vec<(K, V)>, current_block: Option<u64>) {
        let mut batch = WriteBatch::default();

        for (key, value) in changes {
            let serialized_key = bincode::serialize(&key).unwrap();
            let serialized_value = bincode::serialize(&value).unwrap();
            // Add each serialized key-value pair to the batch
            batch.put(serialized_key, serialized_value);
        }

        if let Some(current_block) = current_block {
            let serialized_block_key = bincode::serialize(&"current_block").unwrap();
            let serialized_block_value = bincode::serialize(&current_block).unwrap();
            batch.put(serialized_block_key, serialized_block_value);
        }

        // Execute the batch operation atomically
        self.db.write(batch).unwrap();
    }

    /// inserts data but keep a block as key pointing to the keys inserted in a given block
    /// this makes for faster search based on block_number, ergo index
    pub fn insert_batch_indexed(&self, changes: Vec<(K, V)>, current_block: u64) {
        let mut batch = WriteBatch::default();

        let mut keys = vec![];

        for (key, value) in changes {
            let serialized_key = bincode::serialize(&key).unwrap();
            let serialized_value = bincode::serialize(&value).unwrap();

            keys.push(key);

            // Add each serialized key-value pair to the batch
            batch.put(serialized_key, serialized_value);
        }

        let serialized_block_value = bincode::serialize(&current_block).unwrap();
        let serialized_keys = bincode::serialize(&keys).unwrap();
        batch.put(serialized_block_value, serialized_keys);

        // Execute the batch operation atomically
        self.db.write(batch).unwrap();
    }

    // Deletes an entry from the database by key
    pub fn delete(&self, key: &K) -> Result<()> {
        let serialized_key = bincode::serialize(key)?;
        self.db.delete(serialized_key)?;
        Ok(())
    }

    // Deletes an entry from the database by key
    pub fn delete_index(&self, key: u64) -> Result<()> {
        let serialized_key = bincode::serialize(&key)?;
        //XXX check if value is a vec that can be deserialized as a safety measure
        self.db.delete(serialized_key)?;
        Ok(())
    }

    // Custom method that combines entry and or_insert_with from a HashMap
    pub fn entry_or_insert_with<F>(&self, key: K, default: F) -> V
    where
        F: FnOnce() -> V,
    {
        match self.get(&key) {
            Some(value) => value,
            None => {
                let new_value = default();
                self.insert(key, new_value.clone());
                new_value
            }
        }
    }

    pub fn iter_start(&self) -> RocksDBIterator<K, V> {
        let iter = self.db.iterator(IteratorMode::Start);
        RocksDBIterator::<K, V>::new(iter)
    }

    pub fn iter_end(&self) -> RocksDBIterator<K, V> {
        let iter = self.db.iterator(IteratorMode::End);
        RocksDBIterator::<K, V>::new(iter)
    }

    pub fn indexed_iter_end(&self) -> IndexedRocksDBIterator<K> {
        let iter = self.db.iterator(IteratorMode::End);
        IndexedRocksDBIterator::<K>::new(iter)
    }

    pub fn iter_from<P: Serialize + for<'de> Deserialize<'de> + std::hash::Hash + Eq>(
        &self,
        key_prefix: P,
        direction: rocksdb::Direction,
    ) -> RocksDBIterator<K, V> {
        let serialized_key = bincode::serialize(&key_prefix).unwrap();
        let iter = self.db.iterator(IteratorMode::From(&serialized_key, direction));
        RocksDBIterator::<K, V>::new(iter)
    }

    pub fn last_index(&self) -> Option<(u64, Vec<K>)> {
        let iter = self.db.iterator(IteratorMode::End);
        IndexedRocksDBIterator::<K>::new(iter).next()
    }

    pub fn last(&self) -> Option<(K, V)> {
        let mut iter = self.db.iterator(IteratorMode::End);
        if let Some(Ok((k, v))) = iter.next() {
            let key = bincode::deserialize(&k).unwrap();
            let value = bincode::deserialize(&v).unwrap();
            Some((key, value))
        } else {
            None
        }
    }
}

pub struct RocksDBIterator<'a, K, V> {
    iter: DBIteratorWithThreadMode<'a, DB>,
    _marker: PhantomData<(K, V)>,
}

impl<'a, K: Serialize + for<'de> Deserialize<'de> + std::hash::Hash + Eq, V: Serialize + for<'de> Deserialize<'de> + Clone> RocksDBIterator<'a, K, V> {
    pub fn new(iter: DBIteratorWithThreadMode<'a, DB>) -> Self {
        Self { iter, _marker: PhantomData }
    }
}

/// Custom iterator for navigating RocksDB entries.
///
/// This iterator is designed to skip over specific keys used for internal control purposes, such as:
/// - `"current_block"`: Used to indicate the current block number in the database.
/// - Keys representing index keys (if deserialized as `u64`): Used for various indexing purposes.
///
/// The iterator will:
/// - Ignore any entries where the key is `"current_block"`.
/// - Attempt to deserialize all other keys to the generic type `K`. If deserialization fails, it assumes
///   the key might be an index key or improperly formatted, and skips it.
impl<'a, K: Serialize + for<'de> Deserialize<'de> + std::hash::Hash + Eq, V: Serialize + for<'de> Deserialize<'de> + Clone> Iterator
    for RocksDBIterator<'a, K, V>
{
    type Item = (K, V);

    /// Retrieves the next key-value pair from the database, skipping over special control keys and
    /// potentially misformatted keys.
    ///
    /// Returns:
    /// - `Some((K, V))` if a valid key-value pair is found.
    /// - `None` if there are no more items to process, or if only special/control keys remain.
    fn next(&mut self) -> Option<Self::Item> {
        for key_value_result in self.iter.by_ref() {
            let Ok((key, value)) = key_value_result else { continue };

            // Check if the key is a special 'current_block' key and skip it
            if key == bincode::serialize(&"current_block").unwrap().into_boxed_slice() {
                continue; // Move to the next key-value pair
            }

            // Attempt to deserialize the key to type `K`
            if let Ok(deserialized_key) = bincode::deserialize::<K>(&key) {
                // Attempt to deserialize the value to type `V`
                if let Ok(deserialized_value) = bincode::deserialize::<V>(&value) {
                    // Return the deserialized key-value pair if both are successful
                    return Some((deserialized_key, deserialized_value));
                }
            }
            // If deserialization fails, continue to the next item
        }
        // Return None if all items are processed or if all remaining items fail conditions
        None
    }
}

impl<'a, K: Serialize + for<'de> Deserialize<'de> + std::hash::Hash + Eq> IndexedRocksDBIterator<'a, K> {
    pub fn new(iter: DBIteratorWithThreadMode<'a, DB>) -> Self {
        Self { iter, _marker: PhantomData }
    }
}

pub struct IndexedRocksDBIterator<'a, K> {
    iter: DBIteratorWithThreadMode<'a, DB>,
    _marker: PhantomData<Vec<K>>,
}

impl<'a, K: Serialize + for<'de> Deserialize<'de> + std::hash::Hash + Eq> Iterator for IndexedRocksDBIterator<'a, K> {
    type Item = (u64, Vec<K>);

    fn next(&mut self) -> Option<Self::Item> {
        for key_value_result in self.iter.by_ref() {
            let Ok((key, value)) = key_value_result else { continue };

            if let Ok(index_key) = bincode::deserialize::<u64>(&key) {
                if let Ok(index_values) = bincode::deserialize::<Vec<K>>(&value) {
                    return Some((index_key, index_values));
                }
            }
        }
        None
    }
}
