use std::marker::PhantomData;

use anyhow::Result;
use rocksdb::BlockBasedOptions;
use rocksdb::DBIteratorWithThreadMode;
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

        opts.create_if_missing(true);
        opts.increase_parallelism(16);

        match config {
            DbConfig::LargeSSTFiles => {
                // Adjusting for large SST files
                opts.set_target_file_size_base(256 * 1024 * 1024); // 128MB
                opts.set_max_write_buffer_number(4);
                opts.set_write_buffer_size(64 * 1024 * 1024); // 64MB
                opts.set_max_bytes_for_level_base(512 * 1024 * 1024); // 512MB
                opts.set_max_open_files(1000);
            }
            DbConfig::Default => {
                let mut block_based_options = BlockBasedOptions::default();
                block_based_options.set_ribbon_filter(15.5); // https://github.com/facebook/rocksdb/wiki/RocksDB-Bloom-Filter
                opts.set_block_based_table_factory(&block_based_options);

                opts.set_allow_concurrent_memtable_write(true);
                opts.set_enable_write_thread_adaptive_yield(true);

                let transform = rocksdb::SliceTransform::create_fixed_prefix(10);
                opts.set_prefix_extractor(transform);
                opts.set_memtable_prefix_bloom_ratio(0.2);

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

        let db = DB::open(&opts, db_path)?;

        Ok(RocksDb { db, _marker: PhantomData })
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

    pub fn insert_batch(&self, changes: Vec<(K, V)>, current_block: Option<i64>) {
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
