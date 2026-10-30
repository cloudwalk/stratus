use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::path::PathBuf;

use anyhow::anyhow;
use serde::Deserialize;

use crate::eth::executor::TransactionExecution;
use crate::eth::storage::StorageError;
use crate::eth::types::BlockInfo;
use crate::eth::types::BlockNumber;
use crate::eth::types::TransactionInput;
use crate::globals::DATABASE_PATH;

pub struct JournalBlock {
    pub header: BlockInfo,
    pub transactions: Vec<TransactionInput>,
}

const JOURNAL_DIRECTORY: &str = "transaction-journal";

fn journal_directory() -> PathBuf {
    // permanent storage sets the database path during its init, so this only fails
    // when a journal is opened before the permanent storage exists
    #[allow(clippy::expect_used)]
    DATABASE_PATH
        .get()
        .expect("database path must be anchored by permanent storage init")
        .join(JOURNAL_DIRECTORY)
}

/// Persists ordered transaction inputs until their block reaches durable permanent storage.
#[derive(Debug)]
pub struct TransactionJournal {
    file: Option<File>,
    number: BlockNumber,
}

impl TransactionJournal {
    /// Opens the journal for a block, resuming the file left by a previous run when one exists.
    pub fn open(block_info: BlockInfo) -> anyhow::Result<Self, StorageError> {
        fs::create_dir_all(journal_directory()).map_err(|err| StorageError::Journal { err: anyhow!(err) })?;
        let number = block_info.number;
        let path = Self::path(number);
        let file = if path.exists() {
            OpenOptions::new()
                .append(true)
                .open(path)
                .map_err(|err| StorageError::Journal { err: anyhow!(err) })
        } else {
            Self::start_block(block_info)
        }?;
        Ok(Self { file: Some(file), number })
    }

    fn path(number: BlockNumber) -> PathBuf {
        journal_directory().join(format!("{:020}.journal", number.as_u64()))
    }

    fn start_block(header: BlockInfo) -> Result<File, StorageError> {
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
        let file = self.file.as_mut().ok_or_else(|| StorageError::Journal {
            err: anyhow!("journal file for block {} is closed", self.number),
        })?;
        serde_json::to_writer(file, &transaction).map_err(|err| StorageError::Journal { err: anyhow!(err) })
    }

    /// Reads the block recorded in this journal.
    pub fn block(&self) -> anyhow::Result<Option<JournalBlock>> {
        let mut file = File::open(Self::path(self.number))?;
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

    /// Closes the journal file and removes it from disk.
    pub fn retire(&mut self) -> Result<(), StorageError> {
        self.file = None;
        let path = Self::path(self.number);
        path.exists()
            .then(|| fs::remove_file(path))
            .transpose()
            .map(|_| ())
            .map_err(|err| StorageError::Journal { err: anyhow!(err) })
    }

    /// A journal with no journal files for later blocks belongs to the pending block.
    pub fn is_live(&self) -> bool {
        self.newer_journal_count() == 0
    }

    /// Lists the blocks with a journal file, in ascending order.
    pub fn journaled_blocks() -> anyhow::Result<Vec<BlockNumber>> {
        let entries = match fs::read_dir(journal_directory()) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err.into()),
        };
        let mut numbers = entries.filter_map(|entry| Self::file_block_number(entry.ok()?.path())).collect::<Vec<_>>();
        numbers.sort();
        Ok(numbers)
    }

    /// Reads the block number encoded in a journal file name.
    fn file_block_number(path: PathBuf) -> Option<BlockNumber> {
        if path.extension().is_some_and(|ext| ext == "journal") {
            path.file_stem()?.to_str()?.parse::<u64>().ok().map(BlockNumber::from)
        } else {
            None
        }
    }

    /// Counts journal files for blocks newer than this journal's block.
    fn newer_journal_count(&self) -> usize {
        fs::read_dir(journal_directory())
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter(|entry| Self::file_block_number(entry.path()).is_some_and(|other| other > self.number))
                    .count()
            })
            .unwrap_or(0)
    }

    fn read_record(file: &mut File) -> anyhow::Result<Option<TransactionInput>> {
        match TransactionInput::deserialize(&mut serde_json::Deserializer::from_reader(&mut *file)) {
            Ok(transaction) => Ok(Some(transaction)),
            Err(err) if err.is_eof() => {
                tracing::error!(?err, "journal record is incomplete, ignoring as to not fail startup");
                Ok(None)
            }
            Err(err) => Err(err.into()),
        }
    }
}

impl Drop for TransactionJournal {
    /// Removes the journal file once journals for two later blocks exist: that pair is the committed
    /// latest block and the pending block, so this block is already durable in permanent storage.
    fn drop(&mut self) {
        if self.newer_journal_count() >= 2 {
            let _ = self.retire().inspect_err(|err| tracing::error!(?err, "failed to retire journal on drop"));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::fs::{self};
    use std::path::PathBuf;

    use crate::eth::storage::temporary::journal::TransactionJournal;
    use crate::eth::types::BlockInfo;
    use crate::eth::types::TransactionInput;
    use crate::eth::types::UnixTimeNow;
    use crate::globals::DATABASE_PATH;

    /// Anchors the database path to a temporary directory and returns the journal file path for a block.
    /// The block numbers are near `u64::MAX` so parallel storage tests never write to these files.
    fn journal_file(block_number: u64) -> PathBuf {
        let _ = DATABASE_PATH.set(tempfile::tempdir().unwrap().keep());
        let directory = super::journal_directory();
        fs::create_dir_all(&directory).unwrap();
        super::TransactionJournal::path(block_number.into())
    }

    #[test]
    fn header_truncation_ends_journal_read() {
        const BLOCK_NUMBER: u64 = u64::MAX;
        let header = BlockInfo {
            number: BLOCK_NUMBER.into(),
            timestamp: UnixTimeNow(123.into()),
        };
        let path = journal_file(BLOCK_NUMBER);
        let journal = TransactionJournal::open(header).unwrap();

        let bytes = serde_json::to_vec(&header).unwrap();
        for length in 0..bytes.len() {
            fs::write(&path, &bytes[..length]).unwrap();
            assert!(journal.block().unwrap().is_none(), "length {length}");
        }
        fs::write(&path, bytes).unwrap();
        let block = journal.block().unwrap().unwrap();
        assert_eq!(block.header.number, header.number);
        assert_eq!(block.header.timestamp, header.timestamp);
        assert!(block.transactions.is_empty());
    }

    #[test]
    fn transaction_truncation_preserves_complete_entries_in_order() {
        const BLOCK_NUMBER: u64 = u64::MAX - 1;
        let header = BlockInfo {
            number: BLOCK_NUMBER.into(),
            timestamp: UnixTimeNow(123.into()),
        };
        let path = journal_file(BLOCK_NUMBER);

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
        let journal = TransactionJournal::open(header).unwrap();
        let bytes = fs::read(&path).unwrap();

        let block = journal.block().unwrap().unwrap();
        assert_eq!(block.transactions, vec![first.clone(), second]);
        for length in header_end..bytes.len() {
            fs::write(&path, &bytes[..length]).unwrap();
            let block = journal.block().unwrap().unwrap();
            let expected = if length < first_end { vec![] } else { vec![first.clone()] };
            assert_eq!(block.transactions, expected, "length {length}");
        }
    }
}
