//! RocksDB handling of column families.
//!
//! Check `RocksCfRef` docs for more details.

use std::fmt::Debug;
use std::iter;
use std::marker::PhantomData;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use rocksdb::ColumnFamilyRef;
use rocksdb::DB;
use rocksdb::DBIteratorWithThreadMode;
use rocksdb::IteratorMode;
use rocksdb::ReadOptions;
use rocksdb::WriteBatch;
use serde::Deserialize;
use serde::Serialize;

use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;
#[cfg(feature = "rocks_metrics")]
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
pub struct RocksCfRef<'a, K, V> {
    db: Arc<DB>,
    column_family_name: String,
    pub column_family: ColumnFamilyRef<'a>,
    _marker: PhantomData<(K, V)>,
}

impl<'a, K, V> RocksCfRef<'a, K, V>
where
    K: bincode::Encode + bincode::Decode<()> + Serialize + for<'de> Deserialize<'de> + Debug + std::hash::Hash + Eq + SerializeDeserializeWithContext,
    V: bincode::Encode + bincode::Decode<()> + Serialize + for<'de> Deserialize<'de> + Debug + Clone + SerializeDeserializeWithContext,
{
    /// Create Column Family reference struct.
    pub fn new(db: &'a Arc<DB>, column_family: &str) -> Result<Self> {
        let Some(cf) = db.cf_handle(column_family) else {
            return Err(anyhow!(
                "can't find column family '{}' in database! check if CFs are configured properly when creating/opening the DB",
                column_family,
            ));
        };

        let this = Self {
            db: Arc::clone(db),
            column_family_name: column_family.to_string(),
            column_family: cf,
            _marker: PhantomData,
        };

        Ok(this)
    }

    pub fn db(&self) -> &DB {
        &self.db
    }

    // Clears the database
    #[cfg(feature = "dev")]
    pub fn clear(&self) -> Result<()> {
        // try clearing everything
        let first = self.db.iterator_cf(&self.column_family, IteratorMode::Start).next();
        let last = self.db.iterator_cf(&self.column_family, IteratorMode::End).next();
        if let (Some(Ok((first_key, _))), Some(Ok((last_key, _)))) = (first, last) {
            self.db
                .delete_range_cf(&self.column_family, first_key, last_key)
                .context("when deleting elements in range")?;
        }

        // clear left-overs
        let mut batch = WriteBatch::default();
        for item in self.db.iterator_cf(&self.column_family, IteratorMode::Start) {
            let (key, _) = item.context("when reading an element from the iterator")?;
            batch.delete_cf(&self.column_family, key);
        }
        self.apply_batch_with_context(batch)?;
        Ok(())
    }

    pub fn get(&self, key: &K) -> Result<Option<V>> {
        self.get_impl(key)
            .with_context(|| format!("when trying to read value from CF: '{}'", self.column_family_name))
    }

    #[inline]
    fn get_impl(&self, key: &K) -> Result<Option<V>> {
        let serialized_key = self.serialize_key_with_context(key)?;

        let Some(value_bytes) = self.db.get_cf(&self.column_family, serialized_key)? else {
            return Ok(None);
        };

        self.deserialize_value_with_context(&value_bytes).map(Some)
    }

    #[allow(dead_code)]
    pub fn multi_get<I>(&self, keys: I) -> Result<Vec<(K, V)>>
    where
        I: IntoIterator<Item = K> + Clone,
    {
        let cf_repeated = iter::repeat(&self.column_family);

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

    #[cfg(feature = "dev")]
    pub fn apply_batch_with_context(&self, batch: WriteBatch) -> Result<()> {
        self.db
            .write(batch)
            .with_context(|| format!("failed to apply operation batch to column family '{}'", self.column_family_name))
    }

    pub fn prepare_batch_insertion<I>(&self, insertions: I, batch: &mut WriteBatch) -> Result<()>
    where
        I: IntoIterator<Item = (K, V)>,
    {
        self.prepare_batch_insertion_impl(insertions, batch)
            .with_context(|| format!("failed to prepare batch insertion for CF: '{}'", self.column_family_name))
    }

    #[inline]
    fn prepare_batch_insertion_impl<I>(&self, insertions: I, batch: &mut WriteBatch) -> Result<()>
    where
        I: IntoIterator<Item = (K, V)>,
    {
        for (key, value) in insertions {
            let serialized_key = self.serialize_key_with_context(&key)?;
            let serialized_value = self.serialize_value_with_context(&value)?;
            // add the insertion operation to the batch
            batch.put_cf(&self.column_family, serialized_key, serialized_value);
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn iter_start(&self) -> RocksCfIter<K, V> {
        let iter = self.db.iterator_cf(&self.column_family, IteratorMode::Start);
        RocksCfIter::new(iter, &self.column_family_name)
    }

    pub fn iter_from(&self, start_key: K, direction: rocksdb::Direction) -> Result<RocksCfIter<K, V>> {
        let serialized_key = self.serialize_key_with_context(&start_key)?;

        let iter = self.db.iterator_cf(&self.column_family, IteratorMode::From(&serialized_key, direction));
        Ok(RocksCfIter::new(iter, &self.column_family_name))
    }

    pub fn seek(&self, key: K) -> Result<Option<(K, V)>> {
        let mut opts = ReadOptions::default();
        opts.set_total_order_seek(false);

        let mut iter = self.db.raw_iterator_cf_opt(&self.column_family, opts);

        let serialized_key = self.serialize_key_with_context(&key)?;
        iter.seek(serialized_key);

        if !iter.valid() {
            return Ok(None);
        }
        let Some(key) = iter.key() else { return Ok(None) };
        let Some(value) = iter.value() else { return Ok(None) };

        let deserialized_key =
            K::deserialize_with_context(key).with_context(|| format!("iterator failed to deserialize key in cf '{}'", self.column_family_name))?;
        let deserialized_value =
            V::deserialize_with_context(value).with_context(|| format!("iterator failed to deserialize value in cf '{}'", self.column_family_name))?;

        Ok(Some((deserialized_key, deserialized_value)))
    }

    pub fn first_value(&self) -> Result<Option<V>> {
        let first_pair = self.db.iterator_cf(&self.column_family, IteratorMode::Start).next().transpose()?;

        if let Some((_k, v)) = first_pair {
            self.deserialize_value_with_context(&v).map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn last_key(&self) -> Result<Option<K>> {
        let last_pair = self.db.iterator_cf(&self.column_family, IteratorMode::End).next().transpose()?;

        if let Some((k, _v)) = last_pair {
            self.deserialize_key_with_context(&k).map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn last_value(&self) -> Result<Option<V>> {
        let last_pair = self.db.iterator_cf(&self.column_family, IteratorMode::End).next().transpose()?;
        if let Some((_k, v)) = last_pair {
            self.deserialize_value_with_context(&v).map(Some)
        } else {
            Ok(None)
        }
    }

    #[cfg(feature = "rocks_metrics")]
    pub fn export_metrics(&self) {
        let cur_size_active_mem_table = self
            .db
            .property_int_value_cf(&self.column_family, rocksdb::properties::CUR_SIZE_ACTIVE_MEM_TABLE)
            .unwrap_or_default();
        let cur_size_all_mem_tables = self
            .db
            .property_int_value_cf(&self.column_family, rocksdb::properties::CUR_SIZE_ALL_MEM_TABLES)
            .unwrap_or_default();
        let size_all_mem_tables = self
            .db
            .property_int_value_cf(&self.column_family, rocksdb::properties::SIZE_ALL_MEM_TABLES)
            .unwrap_or_default();
        let block_cache_usage = self
            .db
            .property_int_value_cf(&self.column_family, rocksdb::properties::BLOCK_CACHE_USAGE)
            .unwrap_or_default();
        let block_cache_capacity = self
            .db
            .property_int_value_cf(&self.column_family, rocksdb::properties::BLOCK_CACHE_CAPACITY)
            .unwrap_or_default();
        let background_errors = self
            .db
            .property_int_value_cf(&self.column_family, rocksdb::properties::BACKGROUND_ERRORS)
            .unwrap_or_default();

        let cf_name = &self.column_family_name;
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
        K::deserialize_with_context(key_bytes).with_context(|| format!("when deserializing a key of cf '{}'", self.column_family_name))
    }

    fn deserialize_value_with_context(&self, value_bytes: &[u8]) -> Result<V> {
        V::deserialize_with_context(value_bytes).with_context(|| format!("failed to deserialize value_bytes of cf '{}'", self.column_family_name))
    }

    fn serialize_key_with_context(&self, key: &K) -> Result<Vec<u8>> {
        K::serialize_with_context(key).with_context(|| format!("failed to serialize key of cf '{}'", self.column_family_name))
    }

    fn serialize_value_with_context(&self, value: &V) -> Result<Vec<u8>> {
        V::serialize_with_context(value).with_context(|| format!("failed to serialize value of cf '{}'", self.column_family_name))
    }
}

/// An iterator over K-V pairs in a CF.
pub struct RocksCfIter<'a, K, V> {
    iter: DBIteratorWithThreadMode<'a, DB>,
    column_family: &'a str,
    _marker: PhantomData<(K, V)>,
}

impl<'a, K, V> RocksCfIter<'a, K, V>
where
    K: Serialize + for<'de> Deserialize<'de> + Debug + std::hash::Hash + Eq + SerializeDeserializeWithContext + bincode::Decode<()>,
    V: Serialize + for<'de> Deserialize<'de> + Debug + Clone + SerializeDeserializeWithContext + bincode::Decode<()>,
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

impl<K, V> Iterator for RocksCfIter<'_, K, V>
where
    K: Serialize + for<'de> Deserialize<'de> + Debug + std::hash::Hash + Eq + SerializeDeserializeWithContext + bincode::Decode<()>,
    V: Serialize + for<'de> Deserialize<'de> + Debug + Clone + SerializeDeserializeWithContext + bincode::Decode<()>,
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

        let deserialized_key = K::deserialize_with_context(&key).with_context(|| format!("iterator failed to deserialize key in cf '{}'", self.column_family));
        let deserialized_key = match deserialized_key {
            Ok(inner) => inner,
            Err(e) => return Some(Err(e)),
        };

        let deserialized_value =
            V::deserialize_with_context(&value).with_context(|| format!("iterator failed to deserialize value in cf '{}'", self.column_family));
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
pub struct RocksCfKeysIter<'a, K> {
    iter: DBIteratorWithThreadMode<'a, DB>,
    column_family: &'a str,
    _marker: PhantomData<K>,
}

impl<K> Iterator for RocksCfKeysIter<'_, K>
where
    K: Serialize + for<'de> Deserialize<'de> + Debug + Clone + SerializeDeserializeWithContext + bincode::Decode<()>,
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

        let deserialized_key = K::deserialize_with_context(&key).with_context(|| format!("iterator failed to deserialize key in cf '{}'", self.column_family));
        let deserialized_key = match deserialized_key {
            Ok(inner) => inner,
            Err(e) => return Some(Err(e)),
        };

        Some(Ok(deserialized_key))
    }
}
