use core::fmt;
use revm::primitives::Address;
use rocksdb::{Options, WriteBatch, DB};
use std::sync::Arc;
use std::marker::PhantomData;
use rocksdb::{IteratorMode};
use serde::{Serialize, Deserialize};
use serde_json;
use anyhow::Result;

use super::hybrid_state::AccountInfo;

// A generic struct that abstracts over key-value pairs stored in RocksDB.
pub struct RocksDb<K, V> {
    db: Arc<DB>,
    _marker: PhantomData<(K, V)>,
}

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

    pub fn get(&self, key: &K) -> Option<V> {
        let serialized_key = match serde_json::to_vec(key) {
            Ok(serialized_key) => serialized_key,
            Err(_) => return None,
        };
        let value_bytes = match self.db.get(&serialized_key){
            Ok(Some(value_bytes)) => Some(value_bytes),
            Ok(None) => None,
            Err(_) => None,
        };
        match value_bytes {
            Some(value_bytes) => {
                match serde_json::from_slice(&value_bytes){
                    Ok(value) => Some(value),
                    Err(_) => None,
                }
            },
            None => None,
        }
    }

    // Mimics the 'insert' functionality of a HashMap
    pub fn insert(&self, key: K, value: V) -> Option<V> {
        let serialized_key = serde_json::to_vec(&key).unwrap(); //XXX this is trully a reason for panic, but maybe we can figure a way to serialize
        let prev_value = self.get(&key);
        let serialized_value = serde_json::to_vec(&value).unwrap();
        self.db.put(&serialized_key, &serialized_value).unwrap();
        prev_value
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
                self.insert(key, new_value);
                new_value
            },
        }
    }

}

impl<K, V> fmt::Debug for RocksDb<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RocksDb")
         .field("db", &"Arc<DB>")
         .finish()
    }
}
