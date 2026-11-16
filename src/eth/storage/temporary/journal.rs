use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use anyhow::Context;
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
    file: RecordFile,
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

    fn path(number: BlockNumber) -> PathBuf {
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

    /// Iterates over the journals of the blocks after the mined block.
    pub fn journaled_blocks(mined: BlockNumber) -> JournalIter {
        JournalIter {
            mined,
            next: mined.next_block_number(),
        }
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
pub struct RollingJournal(TransactionJournal);

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

/// Iterator over the journals of the blocks after the mined block, opening each one lazily, in block order, up to the
/// first block without a journal.
pub struct JournalIter {
    mined: BlockNumber,
    next: BlockNumber,
}

impl Drop for JournalIter {
    /// Removes the journals of the blocks mined before recovery and moves every other journal left in the journal
    /// directory aside for review: recovery retires each journal it replays, so those are the ones it failed to recover.
    fn drop(&mut self) {
        let Ok(entries) = fs::read_dir(journal_directory()) else { return };
        let directory = journal_directory().join(UNRECOVERED_DIRECTORY);
        let moved_at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

        for path in entries.filter_map(|entry| Some(entry.ok()?.path())) {
            let Some(number) = TransactionJournal::file_block_number(&path) else {
                continue;
            };
            if number <= self.mined {
                let _ = fs::remove_file(&path).inspect_err(|err| tracing::error!(?err, %number, "failed to remove the journal of a mined block"));
                continue;
            }
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

#[cfg(test)]
mod tests {
    use std::fs;

    use parking_lot::Mutex;
    use parking_lot::MutexGuard;

    use super::JournalRecord;
    use super::RollingJournal;
    use super::TransactionJournal;
    use crate::eth::types::BlockInfo;
    use crate::eth::types::BlockNumber;
    use crate::eth::types::TransactionInput;
    use crate::eth::types::UnixTimeNow;
    use crate::globals::DATABASE_PATH;

    /// Serializes the tests sharing the journal directory: dropping a `JournalIter` moves every journal in it.
    static JOURNAL_DIRECTORY: Mutex<()> = Mutex::new(());

    /// Anchors the database path to a temporary directory and takes the journal directory, emptied, for the test.
    fn journal_directory() -> MutexGuard<'static, ()> {
        let guard = JOURNAL_DIRECTORY.lock();
        let _ = DATABASE_PATH.set(tempfile::tempdir().unwrap().keep());
        let _ = fs::remove_dir_all(super::journal_directory());
        fs::create_dir_all(super::journal_directory()).unwrap();
        guard
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
        let _directory = journal_directory();
        let transactions = [transaction(0), transaction(1), transaction(2)];

        let mut writer = TransactionJournal::create(header(1, 123)).unwrap();
        for transaction in &transactions {
            writer.append(&JournalRecord::encode(transaction).unwrap()).unwrap();
        }

        let mut journal = TransactionJournal::open(1.into()).unwrap();
        let block = journal.block().unwrap();
        assert_eq!(block.header.number, header(1, 123).number);
        assert_eq!(block.header.timestamp, header(1, 123).timestamp);
        assert_eq!(block.transactions, transactions.to_vec());
        journal.retire().unwrap();
    }

    #[test]
    fn unreadable_record_keeps_the_transactions_before_it() {
        let _directory = journal_directory();
        let transactions = [transaction(0), transaction(1), transaction(2)];
        let record = |transaction: &TransactionInput| JournalRecord::encode(transaction).unwrap();

        let mut writer = TransactionJournal::create(header(1, 123)).unwrap();
        writer.append(&record(&transactions[0])).unwrap();
        writer.append(&record(&transactions[1])).unwrap();
        writer.file.append(br#"{"transaction_info":"#).unwrap();
        writer.append(&record(&transactions[2])).unwrap();

        let mut journal = TransactionJournal::open(1.into()).unwrap();
        let block = journal.block().unwrap();
        assert_eq!(block.transactions, transactions[..2].to_vec());
        journal.retire().unwrap();
    }

    #[test]
    fn unreadable_header_fails_the_block_read() {
        let _directory = journal_directory();
        fs::write(TransactionJournal::path(1.into()), br#"{"number":"0x"#).unwrap();

        let mut journal = TransactionJournal::open(1.into()).unwrap();
        assert!(journal.block().is_err());
        journal.retire().unwrap();
    }

    #[test]
    fn open_returns_error_when_the_journal_is_missing() {
        let _directory = journal_directory();
        assert!(TransactionJournal::open(1.into()).is_err());
    }

    #[test]
    fn journaled_blocks_opens_the_blocks_after_the_mined_block_up_to_the_first_gap() {
        let _directory = journal_directory();
        for number in [1, 3, 2, 5] {
            TransactionJournal::create(header(number, 123)).unwrap();
        }

        let mut journals = TransactionJournal::journaled_blocks(1.into());
        for expected in [2, 3] {
            let journal = journals.next().unwrap().unwrap();
            assert_eq!(journal.number(), BlockNumber::from(expected));
            journal.retire().unwrap();
        }
        assert!(journals.next().is_none());
    }

    #[test]
    fn dropping_the_iterator_removes_the_mined_journals_and_sets_aside_the_journals_left() {
        let _directory = journal_directory();
        for number in [1, 2, 3, 4, 6] {
            TransactionJournal::create(header(number, 123)).unwrap();
        }
        let expected = fs::read(TransactionJournal::path(3.into())).unwrap();

        let mut journals = TransactionJournal::journaled_blocks(1.into());
        journals.next().unwrap().unwrap().retire().unwrap();
        // a journal that fails to replay stays on disk for the drop to set aside
        journals.next().unwrap().unwrap();
        drop(journals);

        for number in [1, 2, 3, 4, 6] {
            assert!(!TransactionJournal::path(number.into()).exists());
        }
        let unrecovered = fs::read_dir(super::journal_directory().join(super::UNRECOVERED_DIRECTORY))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(unrecovered.len(), 3);
        let set_aside = |number: u64| {
            unrecovered
                .iter()
                .find(|path| path.file_name().unwrap().to_string_lossy().starts_with(&format!("{number:020}-")))
                .unwrap()
        };
        assert_eq!(fs::read(set_aside(3)).unwrap(), expected);
        set_aside(4);
        set_aside(6);
    }

    #[test]
    fn rolling_journal_is_removed_on_drop_once_two_newer_journals_exist() {
        let _directory = journal_directory();
        for number in [1, 2, 3] {
            TransactionJournal::create(header(number, 123)).unwrap();
        }

        drop(RollingJournal(TransactionJournal::open(2.into()).unwrap()));
        assert!(TransactionJournal::path(2.into()).exists());

        drop(RollingJournal(TransactionJournal::open(1.into()).unwrap()));
        assert!(!TransactionJournal::path(1.into()).exists());

        for number in [2, 3] {
            TransactionJournal::open(number.into()).unwrap().retire().unwrap();
        }
    }
}
