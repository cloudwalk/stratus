use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use prost::Message;
use rocksdb::Options;
use rocksdb::DB;

use super::log_entry::LogEntry;
use super::log_entry::LogEntryData;

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

    pub fn save_log_entry(&self, index: u64, term: u64, data: LogEntryData, entry_type: &str) -> Result<()> {
        tracing::debug!(index, term, "Creating {} log entry", entry_type);
        let log_entry = LogEntry { term, index, data };
        tracing::debug!(index = log_entry.index, term = log_entry.term, "{} log entry created", entry_type);
    
        tracing::debug!("Checking for existing {} entry at new index", entry_type);
        match self.get_entry(log_entry.index) {
            Ok(Some(existing_entry)) => {
                if existing_entry.term != log_entry.term {
                    tracing::warn!(index = log_entry.index, "Conflicting entry found, deleting existing entry and all that follow it");
                    self.delete_entries_from(log_entry.index)?;
                }
            }
            Ok(None) => {
                // No existing entry at this index, proceed to save the new entry
            }
            Err(e) => {
                tracing::error!(index = log_entry.index, "Error retrieving entry: {}", e);
                return Err(anyhow::anyhow!("Error retrieving entry: {}", e));
            }
        }
    
        tracing::debug!("Appending new {} log entry", entry_type);
        self.save_entry(&log_entry)
            .map_err(|e| anyhow::anyhow!("Failed to append {} log entry: {}", entry_type, e))
    }
    
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::eth::consensus::tests::factories::*;
    use crate::eth::consensus::LogEntryData;

    fn setup_storage() -> AppendLogEntriesStorage {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path().to_str().expect("Failed to get temp path").to_string();
        AppendLogEntriesStorage::new(Some(temp_path)).unwrap()
    }

    #[test]
    fn test_save_and_get_block_entry() {
        let storage = setup_storage();
        let index = 1;
        let term = 1;
        let log_entry_data = create_mock_log_entry_data_block();
        let log_entry = create_mock_log_entry(index, term, log_entry_data.clone());

        storage.save_entry(&log_entry).unwrap();
        let retrieved_entry = storage.get_entry(index).unwrap().unwrap();

        assert_eq!(retrieved_entry.index, log_entry.index);
        assert_eq!(retrieved_entry.term, log_entry.term);

        if let LogEntryData::BlockEntry(ref block) = retrieved_entry.data {
            if let LogEntryData::BlockEntry(ref expected_block) = log_entry_data {
                assert_eq!(block.hash, expected_block.hash);
                assert_eq!(block.number, expected_block.number);
                assert_eq!(block.parent_hash, expected_block.parent_hash);
                assert_eq!(block.nonce, expected_block.nonce);
                assert_eq!(block.uncle_hash, expected_block.uncle_hash);
                assert_eq!(block.transactions_root, expected_block.transactions_root);
                assert_eq!(block.state_root, expected_block.state_root);
                assert_eq!(block.receipts_root, expected_block.receipts_root);
                assert_eq!(block.miner, expected_block.miner);
                assert_eq!(block.difficulty, expected_block.difficulty);
                assert_eq!(block.total_difficulty, expected_block.total_difficulty);
                assert_eq!(block.extra_data, expected_block.extra_data);
                assert_eq!(block.size, expected_block.size);
                assert_eq!(block.gas_limit, expected_block.gas_limit);
                assert_eq!(block.gas_used, expected_block.gas_used);
                assert_eq!(block.timestamp, expected_block.timestamp);
                assert_eq!(block.bloom, expected_block.bloom);
                assert_eq!(block.author, expected_block.author);
                assert_eq!(block.transaction_hashes, expected_block.transaction_hashes);
            } else {
                panic!("Expected BlockEntry");
            }
        } else {
            panic!("Expected BlockEntry");
        }
    }

    #[test]
    fn test_save_and_get_transaction_executions_entry() {
        let storage = setup_storage();
        let index = 2;
        let term = 1;
        let log_entry_data = create_mock_log_entry_data_transactions();
        let log_entry = create_mock_log_entry(index, term, log_entry_data.clone());

        storage.save_entry(&log_entry).unwrap();
        let retrieved_entry = storage.get_entry(index).unwrap().unwrap();

        assert_eq!(retrieved_entry.index, log_entry.index);
        assert_eq!(retrieved_entry.term, log_entry.term);

        if let LogEntryData::TransactionExecutionEntries(ref executions) = retrieved_entry.data {
            if let LogEntryData::TransactionExecutionEntries(ref expected_executions) = log_entry_data {
                assert_eq!(executions.len(), expected_executions.len());
                for (execution, expected_execution) in executions.iter().zip(expected_executions.iter()) {
                    assert_eq!(execution.hash, expected_execution.hash);
                    assert_eq!(execution.nonce, expected_execution.nonce);
                    assert_eq!(execution.value, expected_execution.value);
                    assert_eq!(execution.gas_price, expected_execution.gas_price);
                    assert_eq!(execution.input, expected_execution.input);
                    assert_eq!(execution.v, expected_execution.v);
                    assert_eq!(execution.r, expected_execution.r);
                    assert_eq!(execution.s, expected_execution.s);
                    assert_eq!(execution.chain_id, expected_execution.chain_id);
                    assert_eq!(execution.result, expected_execution.result);
                    assert_eq!(execution.output, expected_execution.output);
                    assert_eq!(execution.from, expected_execution.from);
                    assert_eq!(execution.to, expected_execution.to);
                    assert_eq!(execution.block_hash, expected_execution.block_hash);
                    assert_eq!(execution.block_number, expected_execution.block_number);
                    assert_eq!(execution.transaction_index, expected_execution.transaction_index);
                    assert_eq!(execution.logs.len(), expected_execution.logs.len());
                    for (log, expected_log) in execution.logs.iter().zip(expected_execution.logs.iter()) {
                        assert_eq!(log.address, expected_log.address);
                        assert_eq!(log.topics, expected_log.topics);
                        assert_eq!(log.data, expected_log.data);
                        assert_eq!(log.log_index, expected_log.log_index);
                        assert_eq!(log.removed, expected_log.removed);
                        assert_eq!(log.transaction_log_index, expected_log.transaction_log_index);
                    }
                    assert_eq!(execution.gas, expected_execution.gas);
                    assert_eq!(execution.receipt_cumulative_gas_used, expected_execution.receipt_cumulative_gas_used);
                    assert_eq!(execution.receipt_gas_used, expected_execution.receipt_gas_used);
                    assert_eq!(execution.receipt_contract_address, expected_execution.receipt_contract_address);
                    assert_eq!(execution.receipt_status, expected_execution.receipt_status);
                    assert_eq!(execution.receipt_logs_bloom, expected_execution.receipt_logs_bloom);
                    assert_eq!(execution.receipt_effective_gas_price, expected_execution.receipt_effective_gas_price);
                }
            } else {
                panic!("Expected TransactionExecutionEntries");
            }
        } else {
            panic!("Expected TransactionExecutionEntries");
        }
    }
}
