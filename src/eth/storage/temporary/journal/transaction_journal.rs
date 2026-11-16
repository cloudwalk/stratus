use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use anyhow::Context;
use serde::Deserialize;

use super::record_file::RecordFile;
use crate::eth::storage::StorageError;
use crate::eth::types::BlockInfo;
use crate::eth::types::BlockNumber;
use crate::eth::types::TransactionInput;
use crate::globals::DATABASE_PATH;

pub struct JournalBlock {
    pub header: BlockInfo,
    pub transactions: Vec<TransactionInput>,
}

/// A transaction input serialized as a journal entry.
///
/// Local transactions are encoded on the submitting thread so the transaction worker only copies the bytes.
#[derive(Debug)]
pub struct JournalRecord(Vec<u8>);

impl JournalRecord {
    pub fn encode(transaction: &TransactionInput) -> Result<Self, StorageError> {
        serde_json::to_vec(transaction).map(Self).map_err(journal_error)
    }
}

const JOURNAL_DIRECTORY: &str = "transaction-journal";

/// Directory inside the journal directory holding the journals left when recovery stops
pub(super) const UNRECOVERED_DIRECTORY: &str = "unrecovered";

pub(super) fn journal_directory() -> PathBuf {
    // permanent storage sets the database directory during its init, so this only fails
    // when a journal is opened before the permanent storage exists
    #[allow(clippy::expect_used)]
    DATABASE_PATH
        .get()
        .expect("database directory must be anchored by permanent storage init")
        .join(JOURNAL_DIRECTORY)
}

fn journal_error(err: impl Into<anyhow::Error>) -> StorageError {
    StorageError::Journal { err: err.into() }
}

/// Persists ordered transaction inputs until their block reaches durable permanent storage.
#[derive(Debug)]
pub struct TransactionJournal {
    pub(super) file: RecordFile,
    number: BlockNumber,
}

impl TransactionJournal {
    /// Creates the journal of a block
    pub fn create(header: BlockInfo) -> Result<Self, StorageError> {
        fs::create_dir_all(journal_directory()).map_err(journal_error)?;
        let mut file = RecordFile::create(&Self::path(header.number)).map_err(journal_error)?;
        file.append(&serde_json::to_vec(&header).map_err(journal_error)?).map_err(journal_error)?;
        Ok(Self { file, number: header.number })
    }

    /// Opens the journal of a block left on disk
    pub fn open(number: BlockNumber) -> Result<Self, StorageError> {
        let file = RecordFile::open(&Self::path(number)).map_err(journal_error)?;
        Ok(Self { file, number })
    }

    /// Returns the block number of the journaled block.
    pub fn number(&self) -> BlockNumber {
        self.number
    }

    pub(super) fn path(number: BlockNumber) -> PathBuf {
        journal_directory().join(format!("{:020}.journal", number.as_u64()))
    }

    pub fn append(&mut self, record: &JournalRecord) -> Result<(), StorageError> {
        self.file.append(&record.0).map_err(journal_error)
    }

    /// Reads the journaled block, with its transactions up to the first unreadable record.
    pub fn block(&mut self) -> anyhow::Result<JournalBlock> {
        let number = self.number;
        let mut records = serde_json::Deserializer::from_reader(&mut self.file);
        let header = BlockInfo::deserialize(&mut records).with_context(|| format!("journal header of block {number} is unreadable"))?;

        let transactions = records
            .into_iter::<TransactionInput>()
            .enumerate()
            .map_while(|(index, transaction)| {
                transaction
                    .inspect_err(|err| tracing::error!(?err, %number, index, "journal record is unreadable, replaying the transactions before it"))
                    .ok()
            })
            .collect();
        Ok(JournalBlock { header, transactions })
    }

    /// Removes the journal file from disk.
    pub fn retire(self) -> Result<(), StorageError> {
        Self::remove_file(self.number)
    }

    /// Removes the journal file of a block from disk when it exists.
    fn remove_file(number: BlockNumber) -> Result<(), StorageError> {
        let path = Self::path(number);
        path.exists().then(|| fs::remove_file(path)).transpose().map(|_| ()).map_err(journal_error)
    }

    /// Iterates over the journals on disk from the lowest block up to the first block without a journal.
    pub fn journaled_blocks() -> anyhow::Result<JournalIter> {
        let entries = match fs::read_dir(journal_directory()) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(JournalIter { next: BlockNumber::ZERO }),
            Err(err) => return Err(err.into()),
        };
        // with no journal on disk, the iterator starts at a block without a journal and yields nothing
        let lowest = entries
            .filter_map(|entry| Self::file_block_number(&entry.ok()?.path()))
            .min()
            .unwrap_or_default();
        Ok(JournalIter { next: lowest })
    }

    /// Reads the block number encoded in a journal file name.
    fn file_block_number(path: &Path) -> Option<BlockNumber> {
        if path.extension().is_some_and(|ext| ext == "journal") {
            path.file_stem()?.to_str()?.parse::<u64>().ok().map(BlockNumber::from)
        } else {
            None
        }
    }
}

/// Journal of the pending block, removed once its block is durable in permanent storage.
#[derive(Debug)]
pub struct RollingJournal(pub(super) TransactionJournal);

impl RollingJournal {
    /// Creates the journal of a block
    pub fn create(header: BlockInfo) -> Result<Self, StorageError> {
        TransactionJournal::create(header).map(Self)
    }

    pub fn append(&mut self, record: &JournalRecord) -> Result<(), StorageError> {
        self.0.append(record)
    }

    /// Counts journal files for blocks newer than this journal's block.
    fn newer_journal_count(&self) -> usize {
        fs::read_dir(journal_directory())
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter(|entry| TransactionJournal::file_block_number(&entry.path()).is_some_and(|other| other > self.0.number))
                    .count()
            })
            .unwrap_or(0)
    }
}

impl Drop for RollingJournal {
    /// Removes the journal file once journals for two later blocks exist: that pair is the committed
    /// latest block and the pending block, so this block is already durable in permanent storage.
    fn drop(&mut self) {
        if self.newer_journal_count() >= 2 {
            let _ = TransactionJournal::remove_file(self.0.number).inspect_err(|err| tracing::error!(?err, "failed to retire journal on drop"));
        }
    }
}

/// Iterator over consecutive journals, opening each one lazily, in block order, up to the first block without a journal.
pub struct JournalIter {
    next: BlockNumber,
}

impl Drop for JournalIter {
    /// Moves every journal left in the journal directory aside for review: recovery retires each journal it replays or
    /// finds already mined, so the journals left are the ones it failed to recover.
    fn drop(&mut self) {
        let Ok(entries) = fs::read_dir(journal_directory()) else { return };
        let directory = journal_directory().join(UNRECOVERED_DIRECTORY);
        let moved_at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

        for path in entries.filter_map(|entry| Some(entry.ok()?.path())) {
            let Some(number) = TransactionJournal::file_block_number(&path) else {
                continue;
            };
            let destination = directory.join(format!("{:020}-{moved_at}.journal", number.as_u64()));
            match fs::create_dir_all(&directory).and_then(|()| fs::rename(&path, &destination)) {
                Ok(()) => tracing::error!(%number, ?destination, "journal set aside for review"),
                Err(err) => tracing::error!(?err, %number, "failed to set aside the journal"),
            }
        }
    }
}

impl Iterator for JournalIter {
    type Item = Result<TransactionJournal, StorageError>;

    fn next(&mut self) -> Option<Self::Item> {
        let number = self.next;
        if !TransactionJournal::path(number).exists() {
            return None;
        }
        self.next = number.next_block_number();
        Some(TransactionJournal::open(number))
    }
}
