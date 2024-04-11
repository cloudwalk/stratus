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
                opts.set_target_file_size_base(64 * 1024 * 1024); // Starting at 64MB for L1

                // Increase the file size multiplier to expand file size at upper levels
                opts.set_target_file_size_multiplier(2); // Each level grows in file size quicker

                // Reduce the number of L0 files that trigger compaction, increasing frequency
                opts.set_level_zero_file_num_compaction_trigger(2);

                // Reduce thresholds for slowing and stopping writes, which forces more frequent compaction
                opts.set_level_zero_slowdown_writes_trigger(10);
                opts.set_level_zero_stop_writes_trigger(20);

                // Increase the max bytes for L1 to allow more data before triggering compaction
                opts.set_max_bytes_for_level_base(512 * 1024 * 1024); // Setting it to 512MB

                // Increase the level multiplier to aggressively increase space at each level
                opts.set_max_bytes_for_level_multiplier(8.0); // Exponential growth of levels is more pronounced

                // Configure block size to optimize for larger blocks, improving sequential read performance
                block_based_options.set_block_size(128 * 1024); // 128KB blocks

                // Increase the number of write buffers to delay flushing, optimizing CPU usage for compaction
                opts.set_max_write_buffer_number(5);
                opts.set_write_buffer_size(128 * 1024 * 1024); // 128MB per write buffer

                // Keep a higher number of open files to accommodate more files being produced by aggressive compaction
                opts.set_max_open_files(2000);

                // Apply more aggressive compression settings, if I/O and CPU permit
                opts.set_compression_per_level(&[
                    rocksdb::DBCompressionType::None, // No compression for L0
                    rocksdb::DBCompressionType::Zstd, // Use Zstd for higher compression from L1 onwards
                ]);
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

    pub fn backup_engine(&self) -> anyhow::Result<BackupEngine> {
        let backup_opts = BackupEngineOptions::new(&self.backup_path()?)?;
        let backup_env = Env::new()?;
        Ok(BackupEngine::open(&backup_opts, &backup_env)?)
    }

    pub fn backup(&self, backup_engine: &mut BackupEngine) -> anyhow::Result<()> {
        backup_engine.create_new_backup(&self.db)?;
        Ok(())
    }

    pub fn restore(&self, backup_engine: &mut BackupEngine) -> anyhow::Result<()> {
        let restore_options = RestoreOptions::default();
        backup_engine.restore_from_latest_backup(&self.db.path(), self.backup_path()?, &restore_options)?;
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

    pub fn get_current_block_number(&self) -> i64 {
        let Ok(serialized_key) = bincode::serialize(&"current_block") else {
            return -1;
        };
        let Ok(Some(value_bytes)) = self.db.get(serialized_key) else { return -1 };

        bincode::deserialize(&value_bytes).ok().unwrap_or(-1)
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

    // Deletes an entry from the database by key
    pub fn delete(&self, key: &K) -> Result<()> {
        let serialized_key = bincode::serialize(key)?;
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

    pub fn iter_from<P: Serialize + for<'de> Deserialize<'de> + std::hash::Hash + Eq>(
        &self,
        key_prefix: P,
        direction: rocksdb::Direction,
    ) -> RocksDBIterator<K, V> {
        let serialized_key = bincode::serialize(&key_prefix).unwrap();
        let iter = self.db.iterator(IteratorMode::From(&serialized_key, direction));
        RocksDBIterator::<K, V>::new(iter)
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

impl<'a, K: Serialize + for<'de> Deserialize<'de> + std::hash::Hash + Eq, V: Serialize + for<'de> Deserialize<'de> + Clone> Iterator
    for RocksDBIterator<'a, K, V>
{
    type Item = (K, V);
    fn next(&mut self) -> Option<Self::Item> {
        let key_value = self.iter.next();
        match key_value {
            Some(key_value) => {
                let (key, value) = key_value.unwrap(); // XXX deal with the result

                if key == bincode::serialize(&"current_block").unwrap().into_boxed_slice() {
                    return self.next();
                }

                let key: K = bincode::deserialize(&key).unwrap();
                let value: V = bincode::deserialize(&value).unwrap();
                Some((key, value))
            }
            None => None,
        }
    }
}
