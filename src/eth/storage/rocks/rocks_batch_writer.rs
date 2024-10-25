use std::fmt::Debug;
use std::mem;

use anyhow::Context;
use rocksdb::WriteBatch;
use rocksdb::WriteOptions;
use rocksdb::DB;
use serde::Deserialize;
use serde::Serialize;

use super::rocks_cf::RocksCfRef;

pub fn write_in_batch_for_multiple_cfs_impl(db: &DB, batch: WriteBatch) -> anyhow::Result<()> {
    let batch_len = batch.len();
    let mut options = WriteOptions::default();
    // By default, each write to rocksdb is asynchronous: it returns after pushing
    // the write from the process into the operating system (buffer cache).
    // This option enables sync write to ensure data is persisted to disk before
    // returning, preventing potential data loss in case of system failure.
    options.set_sync(true);
    db.write_opt(batch, &options)
        .context("failed to write in batch to (possibly) multiple column families")
        .inspect_err(|e| {
            tracing::error!(reason = ?e, batch_len, "failed to write batch to DB");
        })
}

/// A writer that automatically flushes the batch when it exhausts capacity, supports multiple CFs.
///
/// Similar to `io::BufWriter`.
#[allow(dead_code)]
pub struct BufferedBatchWriter {
    len: usize,
    capacity: usize,
    batch: WriteBatch,
}

#[allow(dead_code)]
impl BufferedBatchWriter {
    pub fn new(capacity: usize) -> Self {
        Self {
            len: 0,
            capacity,
            batch: WriteBatch::default(),
        }
    }

    pub fn insert<K, V>(&mut self, cf_ref: &RocksCfRef<K, V>, key: K, value: V) -> anyhow::Result<()>
    where
        K: Serialize + for<'de> Deserialize<'de> + Debug + std::hash::Hash + Eq,
        V: Serialize + for<'de> Deserialize<'de> + Debug + Clone,
    {
        self.len += 1;
        cf_ref.prepare_batch_insertion([(key, value)], &mut self.batch)?;
        if self.len >= self.capacity {
            self.flush(cf_ref.db())?;
        }
        Ok(())
    }

    pub fn flush(&mut self, db: &DB) -> anyhow::Result<()> {
        if self.len == 0 {
            return Ok(());
        }
        let batch = mem::take(&mut self.batch);
        write_in_batch_for_multiple_cfs_impl(db, batch)?;
        self.len = 0;
        Ok(())
    }
}

impl Drop for BufferedBatchWriter {
    fn drop(&mut self) {
        if self.len > 0 {
            tracing::error!(elements_remaining = %self.len, "BufferedBatchWriter dropped with elements not flushed");
        }
    }
}
