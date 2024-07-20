//! RocksDB handling of column families.
//!
//! Check `RocksCfRef` docs for more details.

use std::fmt::Debug;
use std::iter;
use std::marker::PhantomData;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use const_hex::hex;
use rocksdb::BoundColumnFamily;
use rocksdb::DBIteratorWithThreadMode;
use rocksdb::IteratorMode;
use rocksdb::WriteBatch;
use rocksdb::DB;
use serde::Deserialize;
use serde::Serialize;

#[cfg(feature = "metrics")]
use crate::infra::metrics;

/// A RocksDB Column Family (CF) reference.
///
/// Different CFs can hold data of different types, the main purpose of this struct is to help
/// serializing and deserializing to the correct types, and passing the unique handle in every
/// call to the underlying library.
///
/// Note that creating this struct doesn't write a new CF to the database, instead, CFs are created
/// when creating/opening the database (via `rocksdb::DB` or a wrapper). This is just a reference
/// to an already created CF.
#[derive(Clone)]
pub struct RocksCfRef<K, V> {
    db: Arc<DB>,
    column_family: String,
    _marker: PhantomData<(K, V)>,
}

impl<K, V> RocksCfRef<K, V>
where
    K: Serialize + for<'de> Deserialize<'de> + Debug + std::hash::Hash + Eq,
    V: Serialize + for<'de> Deserialize<'de> + Debug + Clone,
{
    /// Create Column Family reference struct.
    pub fn new(db: Arc<DB>, column_family: &str) -> Self {
        let this = Self {
            db,
            column_family: column_family.to_owned(),
            _marker: PhantomData,
        };

        // Guarantee that the referred database does contain the CF in it
        // With this, we'll be able to talk to the DB
        assert!(
            this.handle_checked().is_some(),
            "can't find column family '{}' in database! check if CFs are configured properly when creating/opening the DB",
            this.column_family,
        );

        this
    }

    fn handle_checked(&self) -> Option<Arc<BoundColumnFamily>> {
        self.db.cf_handle(&self.column_family)
    }

    /// Get the necessary handle for any operation in the CF
    fn handle(&self) -> Arc<BoundColumnFamily> {
        match self.handle_checked() {
            Some(handle) => handle,
            None => {
                panic!(
                    "accessing the CF '{}' failed, but it was checked on creation! this should be impossible",
                    self.column_family,
                );
            }
        }
    }

    // Clears the database
    pub fn clear(&self) -> Result<()> {
        let cf = self.handle();

        // try clearing everything
        let first = self.db.iterator_cf(&cf, IteratorMode::Start).next();
        let last = self.db.iterator_cf(&cf, IteratorMode::End).next();
        if let (Some(Ok((first_key, _))), Some(Ok((last_key, _)))) = (first, last) {
            self.db.delete_range_cf(&cf, first_key, last_key).context("when deleting elements in range")?;
        }

        // clear left-overs
        let mut batch = WriteBatch::default();
        for item in self.db.iterator_cf(&cf, IteratorMode::Start) {
            let (key, _) = item.context("when reading an element from the iterator")?; // Handle or unwrap the Result
            batch.delete_cf(&cf, key);
        }
        self.write_batch_with_context(batch)?;
        Ok(())
    }

    pub fn get(&self, key: &K) -> Result<Option<V>> {
        self.get_impl(key)
            .with_context(|| format!("when trying to read value from CF: '{}'", self.column_family))
    }

    #[inline]
    fn get_impl(&self, key: &K) -> Result<Option<V>> {
        let cf = self.handle();

        let serialized_key = bincode::serialize(key).with_context(|| format!("failed to serialize key in DB get query for CF '{}'", self.column_family))?;

        let Some(value_bytes) = self.db.get_cf(&cf, serialized_key)? else {
            return Ok(None);
        };

        bincode::deserialize(&value_bytes).with_context(|| format!("failed to deserialize value bytes: '{}'", hex::encode(value_bytes)))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn multi_get<I>(&self, keys: I) -> anyhow::Result<Vec<(K, V)>>
    where
        I: IntoIterator<Item = K> + Clone,
    {
        let cf = self.handle();
        let cf_repeated = iter::repeat(&cf);

        let serialized_keys_with_cfs = keys
            .clone()
            .into_iter()
            .zip(cf_repeated)
            .map(|(k, cf)| bincode::serialize(&k).map(|k| (cf, k)))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(self
            .db
            .multi_get_cf(serialized_keys_with_cfs)
            .into_iter()
            .zip(keys)
            .filter_map(|(value, key)| {
                if let Ok(Some(value)) = value {
                    // XXX: Maybe we should fail on a failed conversion instead of ignoring
                    // Didn't fix this yet cause it's currently dead_code, nobody is using it
                    let Ok(value) = bincode::deserialize::<V>(&value) else { return None };
                    Some((key, value))
                } else {
                    None
                }
            })
            .collect())
    }

    // Mimics the 'insert' functionality of a HashMap
    pub fn insert(&self, key: K, value: V) -> Result<()> {
        self.insert_impl(key, value)
            .with_context(|| format!("when trying to insert value in CF: '{}'", self.column_family))
    }

    fn insert_impl(&self, key: K, value: V) -> Result<()> {
        let cf = self.handle();

        let serialized_key = bincode::serialize(&key)?;
        let serialized_value = bincode::serialize(&value)?;

        self.db.put_cf(&cf, serialized_key, serialized_value).map_err(Into::into)
    }

    pub fn prepare_batch_insertion<I>(&self, changes: I, batch: &mut WriteBatch)
    where
        I: IntoIterator<Item = (K, V)>,
    {
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

    // Custom method that combines entry and or_insert_with from a HashMap
    pub fn get_or_insert_with<F>(&self, key: K, default: F) -> Result<V>
    where
        F: FnOnce() -> V,
    {
        let value = self.get(&key)?;

        Ok(match value {
            Some(value) => value,
            None => {
                let new_value = default();
                self.insert(key, new_value.clone())?;
                new_value
            }
        })
    }

    pub fn iter_start(&self) -> RocksCfIterator<K, V> {
        let cf = self.handle();

        let iter = self.db.iterator_cf(&cf, IteratorMode::Start);
        RocksCfIterator::<K, V>::new(iter)
    }

    pub fn iter_end(&self) -> RocksCfIterator<K, V> {
        let cf = self.handle();

        let iter = self.db.iterator_cf(&cf, IteratorMode::End);
        RocksCfIterator::<K, V>::new(iter)
    }

    pub fn iter_from<P: Serialize + for<'de> Deserialize<'de> + std::hash::Hash + Eq>(
        &self,
        key_prefix: P,
        direction: rocksdb::Direction,
    ) -> RocksCfIterator<K, V> {
        let cf = self.handle();
        let serialized_key = bincode::serialize(&key_prefix).unwrap();

        let iter = self.db.iterator_cf(&cf, IteratorMode::From(&serialized_key, direction));
        RocksCfIterator::<K, V>::new(iter)
    }

    #[allow(dead_code)]
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

    pub fn last_key(&self) -> Option<K> {
        let cf = self.handle();

        let mut iter = self.db.iterator_cf(&cf, IteratorMode::End);
        if let Some(Ok((k, _v))) = iter.next() {
            let key = bincode::deserialize(&k).unwrap();
            Some(key)
        } else {
            None
        }
    }

    #[cfg(feature = "metrics")]
    pub fn export_metrics(&self) {
        let handle = self.handle();
        let cur_size_active_mem_table = self
            .db
            .property_int_value_cf(&handle, rocksdb::properties::CUR_SIZE_ACTIVE_MEM_TABLE)
            .unwrap_or_default();
        let cur_size_all_mem_tables = self
            .db
            .property_int_value_cf(&handle, rocksdb::properties::CUR_SIZE_ALL_MEM_TABLES)
            .unwrap_or_default();
        let size_all_mem_tables = self
            .db
            .property_int_value_cf(&handle, rocksdb::properties::SIZE_ALL_MEM_TABLES)
            .unwrap_or_default();
        let block_cache_usage = self
            .db
            .property_int_value_cf(&handle, rocksdb::properties::BLOCK_CACHE_USAGE)
            .unwrap_or_default();
        let block_cache_capacity = self
            .db
            .property_int_value_cf(&handle, rocksdb::properties::BLOCK_CACHE_CAPACITY)
            .unwrap_or_default();
        let background_errors = self
            .db
            .property_int_value_cf(&handle, rocksdb::properties::BACKGROUND_ERRORS)
            .unwrap_or_default();

        let db_name = &self.column_family;
        if let Some(cur_size_active_mem_table) = cur_size_active_mem_table {
            metrics::set_rocks_cur_size_active_mem_table(cur_size_active_mem_table, db_name);
        }

        if let Some(cur_size_all_mem_tables) = cur_size_all_mem_tables {
            metrics::set_rocks_cur_size_all_mem_tables(cur_size_all_mem_tables, db_name);
        }

        if let Some(size_all_mem_tables) = size_all_mem_tables {
            metrics::set_rocks_size_all_mem_tables(size_all_mem_tables, db_name);
        }

        if let Some(block_cache_usage) = block_cache_usage {
            metrics::set_rocks_block_cache_usage(block_cache_usage, db_name);
        }
        if let Some(block_cache_capacity) = block_cache_capacity {
            metrics::set_rocks_block_cache_capacity(block_cache_capacity, db_name);
        }
        if let Some(background_errors) = background_errors {
            metrics::set_rocks_background_errors(background_errors, db_name);
        }
    }

    // TODO: add input value to error message
    fn deserialize_key_with_context(&self, key_bytes: &[u8]) -> Result<K> {
        bincode::deserialize(key_bytes)
            .with_context(|| format!("failed to deserialize '{}'", hex::encode(key_bytes)))
            .with_context(|| format!("when deserializing a key of cf '{}'", self.column_family))
    }

    // TODO: add input value to error message
    fn deserialize_value_with_context(&self, value_bytes: &[u8]) -> Result<V> {
        bincode::deserialize(value_bytes)
            .with_context(|| format!("failed to deserialize value_bytes of cf '{}'", self.column_family))
            .with_context(|| format!("failed to deserialize value_bytes of cf '{}'", self.column_family))
    }

    // TODO: add input value to error message
    fn serialize_key_with_context(&self, key: &K) -> Result<Vec<u8>> {
        bincode::serialize(key).with_context(|| format!("failed to serialize key of cf '{}'", self.column_family))
    }

    // TODO: add input value to error message
    fn serialize_value_with_context(&self, value: &V) -> Result<Vec<u8>> {
        bincode::serialize(value).with_context(|| format!("failed to serialize value of cf '{}'", self.column_family))
    }

    /// A helper function just to add error context to the writing operation.
    fn write_batch_with_context(&self, batch: WriteBatch) -> Result<()> {
        self.db
            .write(batch)
            .with_context(|| format!("failed to write batch to column family '{}'", self.column_family))
    }
}

/// An iterator over data in a CF.
pub(super) struct RocksCfIterator<'a, K, V> {
    iter: DBIteratorWithThreadMode<'a, DB>,
    _marker: PhantomData<(K, V)>,
}

impl<'a, K, V> RocksCfIterator<'a, K, V>
where
    K: Serialize + for<'de> Deserialize<'de> + Debug + std::hash::Hash + Eq,
    V: Serialize + for<'de> Deserialize<'de> + Debug + Clone,
{
    fn new(iter: DBIteratorWithThreadMode<'a, DB>) -> Self {
        Self { iter, _marker: PhantomData }
    }
}

/// Custom iterator for navigating RocksDB entries.
impl<'a, K, V> Iterator for RocksCfIterator<'a, K, V>
where
    K: Serialize + for<'de> Deserialize<'de> + Debug + std::hash::Hash + Eq,
    V: Serialize + for<'de> Deserialize<'de> + Debug + Clone,
{
    type Item = (K, V);

    /// Retrieves the next key-value pair from the database.
    ///
    /// Returns:
    /// - `Some((K, V))` if a valid key-value pair is found.
    /// - `None` if there are no more items to process, or if only special/control keys remain.
    fn next(&mut self) -> Option<Self::Item> {
        let next = self.iter.next()?;
        let (key, value) = next.unwrap();

        let deserialized_key = bincode::deserialize::<K>(&key).unwrap();
        let deserialized_value = bincode::deserialize::<V>(&value).unwrap();
        Some((deserialized_key, deserialized_value))
    }
}
