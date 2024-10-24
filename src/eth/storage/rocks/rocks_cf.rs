//! RocksDB handling of column families.
//!
//! Check `RocksCfRef` docs for more details.

use std::fmt::Debug;
use std::iter;
use std::marker::PhantomData;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
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
    pub fn new(db: Arc<DB>, column_family: &str) -> Result<Self> {
        let this = Self {
            db,
            column_family: column_family.to_owned(),
            _marker: PhantomData,
        };

        // Guarantee that the referred database does contain the CF in it
        // With this, we'll be able to talk to the DB
        if this.handle_checked().is_none() {
            return Err(anyhow!(
                "can't find column family '{}' in database! check if CFs are configured properly when creating/opening the DB",
                this.column_family,
            ));
        }

        Ok(this)
    }

    pub fn db(&self) -> &DB {
        &self.db
    }

    fn handle_checked(&self) -> Option<Arc<BoundColumnFamily>> {
        self.db.cf_handle(&self.column_family)
    }

    /// Get the necessary handle for any operation in the CF
    fn handle(&self) -> Arc<BoundColumnFamily> {
        match self.handle_checked() {
            Some(handle) => handle,
            None => {
                unreachable!(
                    "failed to access CF '{}', but it should be checked when creating RocksCfRef",
                    self.column_family,
                );
            }
        }
    }

    // Clears the database
    #[cfg(feature = "dev")]
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
            let (key, _) = item.context("when reading an element from the iterator")?;
            batch.delete_cf(&cf, key);
        }
        self.apply_batch_with_context(batch)?;
        Ok(())
    }

    pub fn get(&self, key: &K) -> Result<Option<V>> {
        self.get_impl(key)
            .with_context(|| format!("when trying to read value from CF: '{}'", self.column_family))
    }

    #[inline]
    fn get_impl(&self, key: &K) -> Result<Option<V>> {
        let cf = self.handle();

        let serialized_key = self.serialize_key_with_context(key)?;

        let Some(value_bytes) = self.db.get_cf(&cf, serialized_key)? else {
            return Ok(None);
        };

        self.deserialize_value_with_context(&value_bytes).map(Some)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn multi_get<I>(&self, keys: I) -> Result<Vec<(K, V)>>
    where
        I: IntoIterator<Item = K> + Clone,
    {
        let cf = self.handle();
        let cf_repeated = iter::repeat(&cf);

        let serialized_keys_with_cfs = keys
            .clone()
            .into_iter()
            .zip(cf_repeated)
            .map(|(k, cf)| self.serialize_key_with_context(&k).map(|k| (cf, k)))
            .collect::<Result<Vec<(_, _)>>>()?;

        self.db
            .multi_get_cf(serialized_keys_with_cfs)
            .into_iter()
            .map(|result| result.context("when reading a value with multi_get"))
            .zip(keys)
            .filter_map(|(result, key)| {
                result.transpose().map(|value_bytes| {
                    value_bytes
                        .and_then(|value_bytes| self.deserialize_value_with_context(&value_bytes))
                        .map(|value| (key, value))
                })
            })
            .collect()
    }

    #[cfg(test)]
    pub fn apply_batch_with_context(&self, batch: WriteBatch) -> Result<()> {
        self.db
            .write(batch)
            .with_context(|| format!("failed to apply operation batch to column family '{}'", self.column_family))
    }

    pub fn prepare_batch_insertion<I>(&self, insertions: I, batch: &mut WriteBatch) -> Result<()>
    where
        I: IntoIterator<Item = (K, V)>,
    {
        self.prepare_batch_insertion_impl(insertions, batch)
            .with_context(|| format!("failed to prepare batch insertion for CF: '{}'", self.column_family))
    }

    #[inline]
    fn prepare_batch_insertion_impl<I>(&self, insertions: I, batch: &mut WriteBatch) -> Result<()>
    where
        I: IntoIterator<Item = (K, V)>,
    {
        let cf = self.handle();

        for (key, value) in insertions {
            let serialized_key = self.serialize_key_with_context(&key)?;
            let serialized_value = self.serialize_value_with_context(&value)?;
            // add the insertion operation to the batch
            batch.put_cf(&cf, serialized_key, serialized_value);
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn iter_start(&self) -> RocksCfIter<K, V> {
        let cf = self.handle();

        let iter = self.db.iterator_cf(&cf, IteratorMode::Start);
        RocksCfIter::new(iter, &self.column_family)
    }

    pub fn iter_from(&self, start_key: K, direction: rocksdb::Direction) -> Result<RocksCfIter<K, V>> {
        let cf = self.handle();
        let serialized_key = self.serialize_key_with_context(&start_key)?;

        let iter = self.db.iterator_cf(&cf, IteratorMode::From(&serialized_key, direction));
        Ok(RocksCfIter::new(iter, &self.column_family))
    }

    pub fn first_value(&self) -> Result<Option<V>> {
        let cf = self.handle();
        let first_pair = self.db.iterator_cf(&cf, IteratorMode::Start).next().transpose()?;

        if let Some((_k, v)) = first_pair {
            self.deserialize_value_with_context(&v).map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn last_key(&self) -> Result<Option<K>> {
        let cf = self.handle();
        let last_pair = self.db.iterator_cf(&cf, IteratorMode::End).next().transpose()?;

        if let Some((k, _v)) = last_pair {
            self.deserialize_key_with_context(&k).map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn last_value(&self) -> Result<Option<V>> {
        let cf = self.handle();
        let last_pair = self.db.iterator_cf(&cf, IteratorMode::End).next().transpose()?;
        if let Some((_k, v)) = last_pair {
            self.deserialize_value_with_context(&v).map(Some)
        } else {
            Ok(None)
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

        let cf_name = &self.column_family;
        if let Some(cur_size_active_mem_table) = cur_size_active_mem_table {
            metrics::set_rocks_cur_size_active_mem_table(cur_size_active_mem_table, cf_name);
        }

        if let Some(cur_size_all_mem_tables) = cur_size_all_mem_tables {
            metrics::set_rocks_cur_size_all_mem_tables(cur_size_all_mem_tables, cf_name);
        }

        if let Some(size_all_mem_tables) = size_all_mem_tables {
            metrics::set_rocks_size_all_mem_tables(size_all_mem_tables, cf_name);
        }

        if let Some(block_cache_usage) = block_cache_usage {
            metrics::set_rocks_block_cache_usage(block_cache_usage, cf_name);
        }
        if let Some(block_cache_capacity) = block_cache_capacity {
            metrics::set_rocks_block_cache_capacity(block_cache_capacity, cf_name);
        }
        if let Some(background_errors) = background_errors {
            metrics::set_rocks_background_errors(background_errors, cf_name);
        }
    }

    fn deserialize_key_with_context(&self, key_bytes: &[u8]) -> Result<K> {
        deserialize_with_context(key_bytes).with_context(|| format!("when deserializing a key of cf '{}'", self.column_family))
    }

    fn deserialize_value_with_context(&self, value_bytes: &[u8]) -> Result<V> {
        deserialize_with_context(value_bytes).with_context(|| format!("failed to deserialize value_bytes of cf '{}'", self.column_family))
    }

    fn serialize_key_with_context(&self, key: &K) -> Result<Vec<u8>> {
        serialize_with_context(key).with_context(|| format!("failed to serialize key of cf '{}'", self.column_family))
    }

    fn serialize_value_with_context(&self, value: &V) -> Result<Vec<u8>> {
        serialize_with_context(value).with_context(|| format!("failed to serialize value of cf '{}'", self.column_family))
    }
}

fn deserialize_with_context<T>(bytes: &[u8]) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    bincode::deserialize::<T>(bytes)
        .with_context(|| format!("failed to deserialize '{}'", hex_fmt::HexFmt(bytes)))
        .with_context(|| format!("failed to deserialize to type '{}'", std::any::type_name::<T>()))
}

fn serialize_with_context<T>(input: T) -> Result<Vec<u8>>
where
    T: Serialize + Debug,
{
    bincode::serialize(&input).with_context(|| format!("failed to serialize '{input:?}'"))
}

/// An iterator over K-V pairs in a CF.
pub(super) struct RocksCfIter<'a, K, V> {
    iter: DBIteratorWithThreadMode<'a, DB>,
    column_family: &'a str,
    _marker: PhantomData<(K, V)>,
}

impl<'a, K, V> RocksCfIter<'a, K, V>
where
    K: Serialize + for<'de> Deserialize<'de> + Debug + std::hash::Hash + Eq,
    V: Serialize + for<'de> Deserialize<'de> + Debug + Clone,
{
    fn new(iter: DBIteratorWithThreadMode<'a, DB>, column_family: &'a str) -> Self {
        Self {
            iter,
            column_family,
            _marker: PhantomData,
        }
    }

    #[allow(dead_code)]
    pub fn keys(self) -> RocksCfKeysIter<'a, K> {
        RocksCfKeysIter {
            iter: self.iter,
            column_family: self.column_family,
            _marker: PhantomData,
        }
    }
}

impl<'a, K, V> Iterator for RocksCfIter<'a, K, V>
where
    K: Serialize + for<'de> Deserialize<'de> + Debug + std::hash::Hash + Eq,
    V: Serialize + for<'de> Deserialize<'de> + Debug + Clone,
{
    type Item = Result<(K, V)>;

    fn next(&mut self) -> Option<Self::Item> {
        let next = self
            .iter
            .next()?
            .with_context(|| format!("rocksdb iterator failed while reading from cf '{}'", self.column_family));

        let (key, value) = match next {
            Ok(next) => next,
            Err(e) => return Some(Err(e)),
        };

        let deserialized_key = deserialize_with_context(&key).with_context(|| format!("iterator failed to deserialize key in cf '{}'", self.column_family));
        let deserialized_key = match deserialized_key {
            Ok(inner) => inner,
            Err(e) => return Some(Err(e)),
        };

        let deserialized_value =
            deserialize_with_context(&value).with_context(|| format!("iterator failed to deserialize value in cf '{}'", self.column_family));
        let deserialized_value = match deserialized_value {
            Ok(inner) => inner,
            Err(e) => return Some(Err(e)),
        };

        Some(Ok((deserialized_key, deserialized_value)))
    }
}

/// An iterator over keys of a CF.
///
/// This iterator doesn't deserialize values, but the underlying RocksDB iterator still reads them.
///
/// Created by calling `.keys()` on a `RocksCfIter`.
#[allow(dead_code)]
pub(super) struct RocksCfKeysIter<'a, K> {
    iter: DBIteratorWithThreadMode<'a, DB>,
    column_family: &'a str,
    _marker: PhantomData<K>,
}

impl<'a, K> Iterator for RocksCfKeysIter<'a, K>
where
    K: Serialize + for<'de> Deserialize<'de> + Debug + Clone,
{
    type Item = Result<K>;

    fn next(&mut self) -> Option<Self::Item> {
        let next = self
            .iter
            .next()?
            .with_context(|| format!("rocksdb iterator failed while reading from cf '{}'", self.column_family));

        let (key, _value) = match next {
            Ok(next) => next,
            Err(e) => return Some(Err(e)),
        };

        let deserialized_key = deserialize_with_context(&key).with_context(|| format!("iterator failed to deserialize key in cf '{}'", self.column_family));
        let deserialized_key = match deserialized_key {
            Ok(inner) => inner,
            Err(e) => return Some(Err(e)),
        };

        Some(Ok(deserialized_key))
    }
}
