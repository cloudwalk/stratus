use rocksdb::{Options, WriteBatch, DB};
use std::sync::Arc;
use std::marker::PhantomData;

// A generic struct that abstracts over key-value pairs stored in RocksDB.
pub struct RocksDb<K, V> {
    db: Arc<DB>,
    _marker: PhantomData<(K, V)>,
}

impl<K, V> RocksDb<K, V> {

}
use rocksdb::{IteratorMode};
use serde::{Serialize, Deserialize};
use serde_json;
use anyhow::Result;

impl<K: Serialize + for<'de> Deserialize<'de> + std::hash::Hash + Eq, V: Serialize + for<'de> Deserialize<'de>> RocksDb<K, V> {
    pub fn new(db_path: &str) -> anyhow::Result<Self> {
        let db = Arc::new(DB::open_default(&db_path)?);
        Ok(RocksDb {
            db,
            _marker: PhantomData,
        })
    }

    // Clears the database
    pub fn clear(&self) -> Result<()> {
        let mut batch = WriteBatch::default();
        for item in self.db.iterator(IteratorMode::Start) {
            let (key, _) = item?; // Handle or unwrap the Result
            batch.delete(key);
        }

        Ok(())
    }

    pub fn get(&self, key: &K) -> Result<Option<V>> {
        let serialized_key = serde_json::to_vec(key)?;
        match self.db.get(serialized_key)? {
            Some(value_bytes) => {
                let value: V = serde_json::from_slice(&value_bytes)?;
                Ok(Some(value))
            },
            None => Ok(None),
        }
    }

    // Mimics the 'insert' functionality of a HashMap
    pub fn insert(&self, key: &K, value: &V) -> Result<Option<V>> {
        let serialized_key = serde_json::to_vec(key)?;
        let prev_value = self.get(key)?;
        let serialized_value = serde_json::to_vec(value)?;
        self.db.put(&serialized_key, &serialized_value)?;
        Ok(prev_value)
    }

    // Custom method that combines entry and or_insert_with from a HashMap
    pub fn entry_or_insert_with<F>(&self, key: K, default: F) -> Result<V>
    where
        F: FnOnce() -> V,
    {
        match self.get(&key)? {
            Some(value) => Ok(value),
            None => {
                let new_value = default();
                self.insert(&key, &new_value)?;
                Ok(new_value)
            },
        }
    }
}
