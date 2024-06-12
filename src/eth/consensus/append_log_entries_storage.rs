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
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
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

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[derive(Default)]
    struct BlockHeader {
        // example fields
    }

    #[derive(Default)]
    struct TransactionExecution {
        // example fields
    }

    #[test]
    fn test_save_and_get_entry() -> Result<()> {
        let tmp_dir = TempDir::new()?;
        let storage = AppendLogEntriesStorage::new(tmp_dir.path())?;

        let log_entry = LogEntry {
            index: 1,
            term: 1,
            data: LogEntryData::BlockHeader(Default::default()),
        };

        storage.save_entry(&log_entry)?;

        let retrieved_entry = storage.get_entry(1)?;
        assert!(retrieved_entry.is_some());
        assert_eq!(retrieved_entry.unwrap().index, 1);
        Ok(())
    }

    #[test]
    fn test_delete_entries_from() -> Result<()> {
        let tmp_dir = TempDir::new()?;
        let storage = AppendLogEntriesStorage::new(tmp_dir.path())?;

        let log_entry1 = LogEntry {
            index: 1,
            term: 1,
            data: LogEntryData::BlockHeader(Default::default()),
        };
        let log_entry2 = LogEntry {
            index: 2,
            term: 1,
            data: LogEntryData::BlockHeader(Default::default()),
        };

        storage.save_entry(&log_entry1)?;
        storage.save_entry(&log_entry2)?;

        storage.delete_entries_from(2)?;

        let retrieved_entry1 = storage.get_entry(1)?;
        let retrieved_entry2 = storage.get_entry(2)?;
        assert!(retrieved_entry1.is_some());
        assert!(retrieved_entry2.is_none());
        Ok(())
    }

    #[test]
    fn test_get_last_entry() -> Result<()> {
        let tmp_dir = TempDir::new()?;
        let storage = AppendLogEntriesStorage::new(tmp_dir.path())?;

        let log_entry1 = LogEntry {
            index: 1,
            term: 1,
            data: LogEntryData::BlockHeader(Default::default()),
        };
        let log_entry2 = LogEntry {
            index: 2,
            term: 1,
            data: LogEntryData::BlockHeader(Default::default()),
        };

        storage.save_entry(&log_entry1)?;
        storage.save_entry(&log_entry2)?;

        let last_entry = storage.get_last_entry()?;
        assert!(last_entry.is_some());
        assert_eq!(last_entry.unwrap().index, 2);
        Ok(())
    }

    #[test]
    fn test_get_last_index() -> Result<()> {
        let tmp_dir = TempDir::new()?;
        let storage = AppendLogEntriesStorage::new(tmp_dir.path())?;

        let log_entry1 = LogEntry {
            index: 1,
            term: 1,
            data: LogEntryData::BlockHeader(Default::default()),
        };
        let log_entry2 = LogEntry {
            index: 2,
            term: 1,
            data: LogEntryData::BlockHeader(Default::default()),
        };

        storage.save_entry(&log_entry1)?;
        storage.save_entry(&log_entry2)?;

        let last_index = storage.get_last_index()?;
        assert_eq!(last_index, 2);
        Ok(())
    }

    #[test]
    fn test_get_last_term() -> Result<()> {
        let tmp_dir = TempDir::new()?;
        let storage = AppendLogEntriesStorage::new(tmp_dir.path())?;

        let log_entry1 = LogEntry {
            index: 1,
            term: 1,
            data: LogEntryData::BlockHeader(Default::default()),
        };
        let log_entry2 = LogEntry {
            index: 2,
            term: 2,
            data: LogEntryData::BlockHeader(Default::default()),
        };

        storage.save_entry(&log_entry1)?;
        storage.save_entry(&log_entry2)?;

        let last_term = storage.get_last_term()?;
        assert_eq!(last_term, 2);
        Ok(())
    }
}
