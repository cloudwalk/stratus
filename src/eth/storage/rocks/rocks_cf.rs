//! RocksDB handling of column families.

use std::marker::PhantomData;
use std::sync::Arc;

use anyhow::Result;
use rocksdb::BoundColumnFamily;
use rocksdb::DBIteratorWithThreadMode;
use rocksdb::IteratorMode;
use rocksdb::Options;
use rocksdb::WriteBatch;
use rocksdb::DB;
use serde::Deserialize;
use serde::Serialize;

/// A Column Family in RocksDB.
///
/// Exposes an API for key-value pair storage.
#[derive(Clone)]
pub struct RocksCf<K, V> {
    pub db: Arc<DB>,
    // TODO: check if we can gather metrics from a Column Family, if not, remove this field
    _opts: Options,
    column_family: String,
    _marker: PhantomData<(K, V)>,
}

impl<K, V> RocksCf<K, V>
where
    K: Serialize + for<'de> Deserialize<'de> + std::hash::Hash + Eq,
    V: Serialize + for<'de> Deserialize<'de> + Clone,
{
    /// Create Column Family for given DB if it doesn't exist.
    pub fn new_cf(db: Arc<DB>, column_family: &str, opts: Options) -> Self {
        Self {
            db,
            column_family: column_family.to_owned(),
            _opts: opts,
            _marker: PhantomData,
        }
    }

    fn handle(&self) -> Arc<BoundColumnFamily> {
        self.db.cf_handle(&self.column_family).unwrap()
    }

    // Clears the database
    pub fn clear(&self) -> Result<()> {
        let cf = self.handle();

        // try clearing everything
        let first = self.db.iterator_cf(&cf, IteratorMode::Start).next();
        let last = self.db.iterator_cf(&cf, IteratorMode::End).next();
        if let (Some(Ok((first_key, _))), Some(Ok((last_key, _)))) = (first, last) {
            self.db.delete_range_cf(&cf, first_key, last_key)?;
        }

        // clear left-overs
        let mut batch = WriteBatch::default();
        for item in self.db.iterator_cf(&cf, IteratorMode::Start) {
            let (key, _) = item?; // Handle or unwrap the Result
            batch.delete_cf(&cf, key);
        }
        self.db.write(batch)?;
        Ok(())
    }

    pub fn get(&self, key: &K) -> Option<V> {
        let Ok(serialized_key) = bincode::serialize(key) else { return None };
        let cf = self.handle();
        let Ok(Some(value_bytes)) = self.db.get_cf(&cf, serialized_key) else {
            return None;
        };

        bincode::deserialize(&value_bytes).ok()
    }

    pub fn multi_get<I>(&self, keys: I) -> anyhow::Result<Vec<(K, V)>>
    where
        I: IntoIterator<Item = K> + Clone,
    {
        let serialized_keys = keys.clone().into_iter().map(|k| bincode::serialize(&k)).collect::<Result<Vec<_>, _>>()?;
        Ok(self
            .db
            .multi_get(serialized_keys)
            .into_iter()
            .zip(keys)
            .filter_map(|(value, key)| {
                if let Ok(Some(value)) = value {
                    let Ok(value) = bincode::deserialize::<V>(&value) else { return None }; // XXX: Maybe we should fail on a failed conversion instead of ignoring;
                    Some((key, value))
                } else {
                    None
                }
            })
            .collect())
    }

    pub fn get_index_block_number(&self) -> u64 {
        self.last_index().map(|(block_number, _)| block_number).unwrap_or(0)
    }

    // Mimics the 'insert' functionality of a HashMap
    pub fn insert(&self, key: K, value: V) {
        let cf = self.handle();

        let serialized_key = bincode::serialize(&key).unwrap();
        let serialized_value = bincode::serialize(&value).unwrap();
        self.db.put_cf(&cf, serialized_key, serialized_value).unwrap();
    }

    pub fn prepare_batch_insertion(&self, changes: Vec<(K, V)>, batch: &mut WriteBatch) {
        let cf = self.handle();

        for (key, value) in changes {
            let serialized_key = bincode::serialize(&key).unwrap();
            let serialized_value = bincode::serialize(&value).unwrap();
            // Add each serialized key-value pair to the batch
            batch.put_cf(&cf, serialized_key, serialized_value);
        }
    }

    // Deletes an entry from the database by key
    pub fn delete(&self, key: &K) -> Result<()> {
        let serialized_key = bincode::serialize(key)?;
        let cf = self.handle();

        self.db.delete_cf(&cf, serialized_key)?;
        Ok(())
    }

    // Deletes an entry from the database by key
    pub fn delete_index(&self, key: u64) -> Result<()> {
        let serialized_key = bincode::serialize(&key)?;
        let cf = self.handle();
        //XXX check if value is a vec that can be deserialized as a safety measure
        self.db.delete_cf(&cf, serialized_key)?;
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
        let cf = self.handle();

        let iter = self.db.iterator_cf(&cf, IteratorMode::Start);
        RocksDBIterator::<K, V>::new(iter)
    }

    pub fn iter_end(&self) -> RocksDBIterator<K, V> {
        let cf = self.handle();

        let iter = self.db.iterator_cf(&cf, IteratorMode::End);
        RocksDBIterator::<K, V>::new(iter)
    }

    pub fn indexed_iter_end(&self) -> IndexedRocksDBIterator<K> {
        let cf = self.handle();

        let iter = self.db.iterator_cf(&cf, IteratorMode::End);
        IndexedRocksDBIterator::<K>::new(iter)
    }

    pub fn iter_from<P: Serialize + for<'de> Deserialize<'de> + std::hash::Hash + Eq>(
        &self,
        key_prefix: P,
        direction: rocksdb::Direction,
    ) -> RocksDBIterator<K, V> {
        let cf = self.handle();
        let serialized_key = bincode::serialize(&key_prefix).unwrap();

        let iter = self.db.iterator_cf(&cf, IteratorMode::From(&serialized_key, direction));
        RocksDBIterator::<K, V>::new(iter)
    }

    pub fn last_index(&self) -> Option<(u64, Vec<K>)> {
        let cf = self.handle();

        let iter = self.db.iterator_cf(&cf, IteratorMode::End);
        IndexedRocksDBIterator::<K>::new(iter).next()
    }

    pub fn last(&self) -> Option<(K, V)> {
        let cf = self.handle();

        let mut iter = self.db.iterator_cf(&cf, IteratorMode::End);
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
