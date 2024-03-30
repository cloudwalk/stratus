use std::marker::PhantomData;

use anyhow::Result;
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

        match config {
            DbConfig::LargeSSTFiles => {
                // Adjusting for large SST files
                opts.set_target_file_size_base(256 * 1024 * 1024); // 128MB
                opts.set_max_write_buffer_number(4);
                opts.set_write_buffer_size(64 * 1024 * 1024); // 64MB
                opts.set_max_bytes_for_level_base(512 * 1024 * 1024); // 512MB
                opts.set_max_open_files(100);
            }
            DbConfig::Default => {} // Default options are already set
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

    // Mimics the 'insert' functionality of a HashMap
    pub fn insert(&self, key: K, value: V) {
        let serialized_key = bincode::serialize(&key).unwrap();
        let serialized_value = bincode::serialize(&value).unwrap();
        self.db.put(serialized_key, serialized_value).unwrap();
    }

    pub fn insert_batch(&self, changes: Vec<(K, V)>) {
        let mut batch = WriteBatch::default();

        for (key, value) in changes {
            let serialized_key = bincode::serialize(&key).unwrap();
            let serialized_value = bincode::serialize(&value).unwrap();
            // Add each serialized key-value pair to the batch
            batch.put(serialized_key, serialized_value);
        }

        // Execute the batch operation atomically
        self.db.write(batch).unwrap();
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

                let key: K = bincode::deserialize(&key).unwrap();
                let value: V = bincode::deserialize(&value).unwrap();
                Some((key, value))
            }
            None => None,
        }
    }
}
