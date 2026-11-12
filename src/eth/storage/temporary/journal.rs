use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use serde::Deserialize;

use crate::eth::storage::StorageError;
use crate::eth::storage::temporary::record_file::RecordFile;
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
const UNRECOVERED_DIRECTORY: &str = "unrecovered";

fn journal_directory() -> PathBuf {
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
    file: Option<RecordFile>,
    number: BlockNumber,
}

impl TransactionJournal {
    /// Creates the journal of a block
    pub fn open(header: BlockInfo) -> Result<Self, StorageError> {
        fs::create_dir_all(journal_directory()).map_err(journal_error)?;
        let mut file = RecordFile::create(&Self::path(header.number)).map_err(journal_error)?;
        file.append(&serde_json::to_vec(&header).map_err(journal_error)?).map_err(journal_error)?;
        Ok(Self {
            file: Some(file),
            number: header.number,
        })
    }

    fn path(number: BlockNumber) -> PathBuf {
        journal_directory().join(format!("{:020}.journal", number.as_u64()))
    }

    fn file(&mut self) -> Result<&mut RecordFile, StorageError> {
        let number = self.number;
        self.file
            .as_mut()
            .ok_or_else(|| journal_error(anyhow::anyhow!("journal file for block {number} is closed")))
    }

    pub fn append(&mut self, record: &JournalRecord) -> Result<(), StorageError> {
        self.file()?.append(&record.0).map_err(journal_error)
    }

    /// Reads the block journaled for `number`, with its transactions up to the first unreadable record.
    /// Returns `None` when the journal holds no readable header.
    pub fn read(number: BlockNumber) -> anyhow::Result<Option<JournalBlock>> {
        let mut records = serde_json::Deserializer::from_reader(RecordFile::open(&Self::path(number))?);

        let header = match BlockInfo::deserialize(&mut records) {
            Ok(header) => header,
            Err(err) => {
                tracing::error!(?err, %number, "journal header is unreadable");
                return Ok(None);
            }
        };

        let transactions = records
            .into_iter::<TransactionInput>()
            .enumerate()
            .map_while(|(index, transaction)| {
                transaction
                    .inspect_err(|err| tracing::error!(?err, %number, index, "journal record is unreadable, replaying the transactions before it"))
                    .ok()
            })
            .collect();
        Ok(Some(JournalBlock { header, transactions }))
    }

    /// Closes the journal file and removes it from disk.
    pub fn retire(&mut self) -> Result<(), StorageError> {
        self.file = None;
        Self::remove(self.number)
    }

    /// Removes the journal file of a block
    pub fn remove(number: BlockNumber) -> Result<(), StorageError> {
        let path = Self::path(number);
        path.exists().then(|| fs::remove_file(path)).transpose().map(|_| ()).map_err(journal_error)
    }

    /// Moves the journal file of a block into the unrecovered directory for review
    pub fn set_aside(number: BlockNumber) -> anyhow::Result<PathBuf> {
        let directory = journal_directory().join(UNRECOVERED_DIRECTORY);
        fs::create_dir_all(&directory)?;
        let moved_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let destination = directory.join(format!("{:020}-{moved_at}.journal", number.as_u64()));
        fs::rename(Self::path(number), &destination)?;
        Ok(destination)
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
    use std::fs;

    use super::JournalRecord;
    use super::TransactionJournal;
    use crate::eth::types::BlockInfo;
    use crate::eth::types::TransactionInput;
    use crate::eth::types::UnixTimeNow;
    use crate::globals::DATABASE_PATH;

    /// Anchors the database path to a temporary directory.
    /// The block numbers are near `u64::MAX` so parallel storage tests never write to these files.
    fn anchor_database_path() {
        let _ = DATABASE_PATH.set(tempfile::tempdir().unwrap().keep());
        fs::create_dir_all(super::journal_directory()).unwrap();
    }

    fn header(number: u64, timestamp: u64) -> BlockInfo {
        BlockInfo {
            number: number.into(),
            timestamp: UnixTimeNow(timestamp.into()),
        }
    }

    fn transaction(nonce: u64) -> TransactionInput {
        let mut transaction = TransactionInput::default();
        transaction.execution_info.nonce = nonce.into();
        transaction
    }

    #[test]
    fn block_is_read_in_order() {
        const BLOCK_NUMBER: u64 = u64::MAX;
        anchor_database_path();
        let transactions = [transaction(0), transaction(1), transaction(2)];

        let mut journal = TransactionJournal::open(header(BLOCK_NUMBER, 123)).unwrap();
        for transaction in &transactions {
            journal.append(&JournalRecord::encode(transaction).unwrap()).unwrap();
        }

        let block = TransactionJournal::read(BLOCK_NUMBER.into()).unwrap().unwrap();
        assert_eq!(block.header.number, header(BLOCK_NUMBER, 123).number);
        assert_eq!(block.header.timestamp, header(BLOCK_NUMBER, 123).timestamp);
        assert_eq!(block.transactions, transactions.to_vec());
        journal.retire().unwrap();
    }

    #[test]
    fn unreadable_record_keeps_the_transactions_before_it() {
        const BLOCK_NUMBER: u64 = u64::MAX - 1;
        anchor_database_path();
        let transactions = [transaction(0), transaction(1), transaction(2)];
        let record = |transaction: &TransactionInput| JournalRecord::encode(transaction).unwrap();

        let mut journal = TransactionJournal::open(header(BLOCK_NUMBER, 123)).unwrap();
        journal.append(&record(&transactions[0])).unwrap();
        journal.append(&record(&transactions[1])).unwrap();
        journal.file().unwrap().append(br#"{"transaction_info":"#).unwrap();
        journal.append(&record(&transactions[2])).unwrap();

        let block = TransactionJournal::read(BLOCK_NUMBER.into()).unwrap().unwrap();
        assert_eq!(block.transactions, transactions[..2].to_vec());
        journal.retire().unwrap();
    }

    #[test]
    fn unreadable_header_reads_as_no_block() {
        const BLOCK_NUMBER: u64 = u64::MAX - 2;
        anchor_database_path();
        fs::write(TransactionJournal::path(BLOCK_NUMBER.into()), br#"{"number":"0x"#).unwrap();

        assert!(TransactionJournal::read(BLOCK_NUMBER.into()).unwrap().is_none());
        TransactionJournal::remove(BLOCK_NUMBER.into()).unwrap();
    }

    #[test]
    fn set_aside_journal_leaves_the_journaled_blocks() {
        const BLOCK_NUMBER: u64 = u64::MAX - 3;
        anchor_database_path();
        // held until the file is moved, since dropping a journal removes its file once newer journals exist
        let journal = TransactionJournal::open(header(BLOCK_NUMBER, 123)).unwrap();
        assert!(TransactionJournal::journaled_blocks().unwrap().contains(&BLOCK_NUMBER.into()));

        let destination = TransactionJournal::set_aside(BLOCK_NUMBER.into()).unwrap();
        assert!(destination.exists());
        assert!(!TransactionJournal::journaled_blocks().unwrap().contains(&BLOCK_NUMBER.into()));
        drop(journal);
    }
}
