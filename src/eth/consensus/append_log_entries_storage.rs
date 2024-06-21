use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use prost::Message;
use rocksdb::Options;
use rocksdb::DB;

use super::log_entry::LogEntry;

pub struct AppendLogEntriesStorage {
    db: DB,
}

impl AppendLogEntriesStorage {
    pub fn new(path: Option<String>) -> Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);

        let path = if let Some(prefix) = path {
            // run some checks on the given prefix
            assert!(!prefix.is_empty(), "given prefix for RocksDB is empty, try not providing the flag");
            if Path::new(&prefix).is_dir() || Path::new(&prefix).iter().count() > 1 {
                tracing::warn!(?prefix, "given prefix for RocksDB might put it in another folder");
            }

            let path = format!("{prefix}-log-entries-rocksdb");
            tracing::info!("starting rocksdb log entries storage - at custom path: '{:?}'", path);
            path
        } else {
            tracing::info!("starting rocksdb log entries storage - at default path: 'data/log-entries-rocksdb'"); // TODO: keep inside data?
            "data/log-entries-rocksdb".to_string()
        };

        let db = DB::open(&opts, path).context("Failed to open RocksDB")?;
        Ok(Self { db })
    }

    pub fn save_entry(&self, entry: &LogEntry) -> Result<()> {
        let mut buf = Vec::new();
        entry.encode(&mut buf).context("Failed to encode log entry")?;
        self.db.put(entry.index.to_be_bytes(), buf).context("Failed to save log entry")?;
        Ok(())
    }

    pub fn get_entry(&self, index: u64) -> Result<Option<LogEntry>> {
        match self.db.get(index.to_be_bytes()).context("Failed to get log entry")? {
            Some(value) => {
                let entry = LogEntry::decode(&*value).context("Failed to decode log entry")?;
                Ok(Some(entry))
            }
            None => Ok(None),
        }
    }

    pub fn delete_entries_from(&self, start_index: u64) -> Result<()> {
        let iter = self
            .db
            .iterator(rocksdb::IteratorMode::From(start_index.to_be_bytes().as_ref(), rocksdb::Direction::Forward));

        for result in iter {
            match result {
                Ok((key, _)) => {
                    self.db.delete(key).context("Failed to delete log entry")?;
                }
                Err(e) => return Err(e).context("Error iterating over log entries"),
            }
        }
        Ok(())
    }

    pub fn get_last_entry(&self) -> Result<Option<LogEntry>> {
        let mut iter = self.db.iterator(rocksdb::IteratorMode::End);
        match iter.next() {
            Some(Ok((_, value))) => LogEntry::decode(&*value).map(Some).context("Failed to decode last log entry"),
            Some(Err(e)) => Err(e).context("Error iterating to get last log entry"),
            None => Ok(None),
        }
    }

    pub fn get_last_index(&self) -> Result<u64> {
        let mut iter = self.db.iterator(rocksdb::IteratorMode::End);
        match iter.next() {
            Some(Ok((key, _))) => {
                let key_vec: Vec<u8> = key.to_vec(); // Convert Box<[u8]> to Vec<u8>
                let index = u64::from_be_bytes(<Vec<u8> as AsRef<[u8]>>::as_ref(&key_vec).try_into().context("Failed to convert key to u64")?);
                Ok(index)
            }
            Some(Err(e)) => Err(e).context("Error iterating to get last index"),
            None => Ok(0),
        }
    }

    pub fn get_last_term(&self) -> Result<u64> {
        match self.get_last_entry()? {
            Some(entry) => Ok(entry.term),
            None => Ok(0), // Default to 0 if not set
        }
    }
}
