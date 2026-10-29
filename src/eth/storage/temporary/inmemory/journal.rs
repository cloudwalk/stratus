use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use parking_lot::Mutex;
use serde::Deserialize;

use crate::eth::executor::Executor;
use crate::eth::executor::TransactionExecution;
use crate::eth::miner::Miner;
use crate::eth::rpc::BlockFilter;
use crate::eth::storage::StorageError;
use crate::eth::storage::StratusStorage;
use crate::eth::types::BlockInfo;
use crate::eth::types::BlockNumber;
use crate::eth::types::TransactionInput;

pub(crate) struct JournalBlock {
    pub header: BlockInfo,
    pub transactions: Vec<TransactionInput>,
}

const BASE_PATH: &str = "./journal";

/// Persists ordered transactions until their block reaches durable permanent storage.
#[derive(Debug, Clone)]
pub struct TransactionJournal {
    file: Arc<Mutex<File>>,
}

impl TransactionJournal {
    pub fn open(block_info: BlockInfo) -> anyhow::Result<Self, StorageError> {
        fs::create_dir_all(BASE_PATH).map_err(|err| StorageError::Journal { err: anyhow!(err) })?;
        Ok(Self {
            file: Arc::new(Mutex::new(Self::start_block(block_info)?)),
        })
    }

    fn path(number: BlockNumber) -> PathBuf {
        Path::new(BASE_PATH).join(format!("{:020}.journal", number.as_u64()))
    }

    pub fn start_block(header: BlockInfo) -> Result<File, StorageError> {
        OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(Self::path(header.number))
            .map_err(|err| StorageError::Journal { err: anyhow!(err) })
            .and_then(|mut file| {
                serde_json::to_writer(&mut file, &header)
                    .map(|_| file)
                    .map_err(|err| StorageError::Journal { err: anyhow!(err) })
            })
    }

    pub fn append(&mut self, execution: &TransactionExecution) -> Result<(), StorageError> {
        // could be a little cheaper if we created a TransactionInputView struct with all borrowed values that serialized the same
        let transaction: TransactionInput = execution.into();
        serde_json::to_writer(&mut *(self.file.lock()), &transaction).map_err(|err| StorageError::Journal { err: anyhow!(err) })
    }

    pub fn retire(number: BlockNumber) -> Result<(), StorageError> {
        let path = Self::path(number);
        path.exists()
            .then(|| fs::remove_file(path))
            .transpose()
            .map(|_| ())
            .map_err(|err| StorageError::Journal { err: anyhow!(err) })
    }

    #[cfg(feature = "dev")]
    pub fn reset(&mut self, header: BlockInfo) {
        self.active = None;
        let result = (|| {
            for entry in fs::read_dir(BASE_PATH)? {
                let path = entry?.path();
                if path.extension().is_some_and(|ext| ext == "journal") {
                    fs::remove_file(path)?;
                }
            }
        })();
        if let Err(err) = result {
            journal_error(err);
        }
        let _ = self.start_block(header);
    }

    fn journal_file_paths() -> anyhow::Result<Vec<PathBuf>> {
        let mut paths = fs::read_dir(BASE_PATH)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        paths.retain(|path| path.extension().is_some_and(|ext| ext == "journal"));
        paths.sort();
        Ok(paths)
    }

    fn read_record(file: &mut File) -> anyhow::Result<Option<TransactionInput>> {
        match TransactionInput::deserialize(&mut serde_json::Deserializer::from_reader(&mut *file)) {
            Ok(transaction) => Ok(Some(transaction)),
            Err(err) if err.is_eof() => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn read_block(path: &Path) -> anyhow::Result<Option<JournalBlock>> {
        let mut file = File::open(path)?;
        let header = match BlockInfo::deserialize(&mut serde_json::Deserializer::from_reader(&mut file)) {
            Ok(header) => header,
            Err(err) if err.is_eof() => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let mut transactions = Vec::new();
        while let Some(transaction) = Self::read_record(&mut file)? {
            transactions.push(transaction);
        }
        Ok(Some(JournalBlock { header, transactions }))
    }

    /// Replays journaled transactions and commits recovered blocks before startup continues.
    pub fn recover_transaction_journal(executor: &Executor, miner: &Miner, storage: &StratusStorage) -> anyhow::Result<()> {
        let paths = Self::journal_file_paths()?;
        for path in paths {
            let Some(journal_block) = TransactionJournal::read_block(&path)? else {
                continue;
            };
            let header = journal_block.header;
            if storage.read_block(BlockFilter::Number(header.number))?.is_none() {
                storage.set_pending_header(header.number, *(header.timestamp));
                for transaction in journal_block.transactions {
                    executor.execute_local_transaction(transaction, None)?;
                }
                miner.mine_local_and_commit()?;
            }
            Self::retire(header.number)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::fs::{self};

    use crate::eth::storage::temporary::inmemory::journal::TransactionJournal;
    use crate::eth::types::BlockInfo;
    use crate::eth::types::TransactionInput;
    use crate::eth::types::UnixTimeNow;

    #[test]
    fn header_truncation_ends_journal_read() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("1.journal");
        let header = BlockInfo {
            number: 1.into(),
            timestamp: UnixTimeNow(123.into()),
        };
        let bytes = serde_json::to_vec(&header).unwrap();
        for length in 0..bytes.len() {
            fs::write(&path, &bytes[..length]).unwrap();
            assert!(TransactionJournal::read_block(&path).unwrap().is_none(), "length {length}");
        }
        fs::write(&path, bytes).unwrap();
        let block = TransactionJournal::read_block(&path).unwrap().unwrap();
        assert_eq!(block.header.number, header.number);
        assert_eq!(block.header.timestamp, header.timestamp);
        assert!(block.transactions.is_empty());
    }

    #[test]
    fn transaction_truncation_preserves_complete_entries_in_order() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("1.journal");
        let header = BlockInfo {
            number: 1.into(),
            timestamp: UnixTimeNow(123.into()),
        };

        let first = TransactionInput::default();
        let mut second = first.clone();

        second.execution_info.nonce = 1.into();
        let mut file = File::create(&path).unwrap();

        serde_json::to_writer(&mut file, &header).unwrap();
        let header_end = file.metadata().unwrap().len() as usize;

        serde_json::to_writer(&mut file, &first).unwrap();
        let first_end = file.metadata().unwrap().len() as usize;

        serde_json::to_writer(&mut file, &second).unwrap();

        drop(file);
        let bytes = fs::read(&path).unwrap();

        let block = TransactionJournal::read_block(&path).unwrap().unwrap();
        assert_eq!(block.transactions, vec![first.clone(), second]);
        for length in header_end..bytes.len() {
            fs::write(&path, &bytes[..length]).unwrap();
            let block = TransactionJournal::read_block(&path).unwrap().unwrap();
            let expected = if length < first_end { vec![] } else { vec![first.clone()] };
            assert_eq!(block.transactions, expected, "length {length}");
        }
    }
}
