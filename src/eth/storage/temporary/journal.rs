use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::BufReader;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use anyhow::Context;
use derive_more::Deref;
use parking_lot::ArcMutexGuard;
use parking_lot::Mutex;
use parking_lot::RawMutex;
use serde::Deserialize;

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
#[derive(Debug, Deref)]
pub struct JournalRecord(#[deref] Vec<u8>);

impl JournalRecord {
    pub fn encode(transaction: &TransactionInput) -> Result<Self, StorageError> {
        json_line(transaction).map(Self)
    }
}

/// Serializes a journal entry as a line of JSON.
fn json_line(value: &impl serde::Serialize) -> Result<Vec<u8>, StorageError> {
    let mut line = serde_json::to_vec(value).map_err(journal_error)?;
    line.push(b'\n');
    Ok(line)
}

/// The journal of the pending block, swapped when the block finishes.
pub type PendingJournal = Arc<Mutex<Option<Arc<RollingJournal>>>>;

/// A transaction's turn to append its record to the journal of the block it was saved in.
///
/// Turns are taken in execution order and hold the pending journal lock, so records reach the journals in execution
/// order and the journal is swapped only after the previous block's records are appended.
#[derive(derive_new::new)]
pub struct JournalTurn(ArcMutexGuard<RawMutex, Option<Arc<RollingJournal>>>);

impl JournalTurn {
    pub fn append(self, record: &JournalRecord) -> Result<(), StorageError> {
        match self.0.as_ref() {
            Some(journal) => journal.append(record),
            None => Ok(()),
        }
    }
}

const JOURNAL_DIRECTORY: &str = "transaction-journal";

/// Directory inside the journal directory holding the journals left when recovery stops.
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
    file: File,
    number: BlockNumber,
}

impl TransactionJournal {
    /// Creates the journal of a block, replacing the file left for the same block.
    pub fn create(header: BlockInfo) -> Result<Self, StorageError> {
        fs::create_dir_all(journal_directory()).map_err(journal_error)?;
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(Self::path(header.number))
            .map_err(journal_error)?;
        file.write_all(&json_line(&header)?).map_err(journal_error)?;
        Ok(Self { file, number: header.number })
    }

    /// Opens the journal of a block left on disk.
    pub fn open(number: BlockNumber) -> Result<Self, StorageError> {
        let file = File::open(Self::path(number)).map_err(journal_error)?;
        Ok(Self { file, number })
    }

    /// Returns the block number of the journaled block.
    pub fn number(&self) -> BlockNumber {
        self.number
    }

    fn path(number: BlockNumber) -> PathBuf {
        journal_directory().join(format!("{:020}.journal", number.as_u64()))
    }

    /// Appends a record.
    pub fn append(&self, record: &JournalRecord) -> Result<(), StorageError> {
        (&self.file).write_all(record).map_err(journal_error)
    }

    /// Reads the journaled block, with its transactions up to the first unreadable record.
    pub fn block(&mut self) -> anyhow::Result<JournalBlock> {
        let number = self.number;
        let mut records = serde_json::Deserializer::from_reader(BufReader::new(&self.file));
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
pub struct RollingJournal(TransactionJournal);

impl RollingJournal {
    /// Creates the journal of a block, replacing the file left for the same block.
    pub fn create(header: BlockInfo) -> Result<Self, StorageError> {
        TransactionJournal::create(header).map(Self)
    }

    /// Appends a record.
    pub fn append(&self, record: &JournalRecord) -> Result<(), StorageError> {
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

    fn header(number: u64) -> BlockInfo {
        BlockInfo {
            number: number.into(),
            timestamp: UnixTimeNow(123.into()),
        }
    }

    fn transaction(nonce: u64) -> TransactionInput {
        let mut transaction = TransactionInput::default();
        transaction.execution_info.nonce = nonce.into();
        transaction
    }

    fn read_block(number: u64) -> anyhow::Result<super::JournalBlock> {
        TransactionJournal::open(number.into()).unwrap().block()
    }

    #[test]
    fn header_truncation_fails_the_block_read() {
        let _directory = journal_directory();
        let path = TransactionJournal::path(1.into());

        let bytes = super::json_line(&header(1)).unwrap();
        let value_end = bytes.len() - 1;
        for length in 0..value_end {
            fs::write(&path, &bytes[..length]).unwrap();
            assert!(read_block(1).is_err(), "length {length}");
        }
        fs::write(&path, bytes).unwrap();
        let block = read_block(1).unwrap();
        assert_eq!(block.header.number, header(1).number);
        assert_eq!(block.header.timestamp, header(1).timestamp);
        assert!(block.transactions.is_empty());
    }

    #[test]
    fn transaction_truncation_preserves_complete_entries_in_order() {
        let _directory = journal_directory();
        let path = TransactionJournal::path(1.into());
        let first = transaction(0);
        let second = transaction(1);

        let mut bytes = super::json_line(&header(1)).unwrap();
        let header_end = bytes.len();
        bytes.extend(JournalRecord::encode(&first).unwrap().iter());
        // a record is complete once its JSON value is, with or without the trailing newline
        let first_end = bytes.len() - 1;
        bytes.extend(JournalRecord::encode(&second).unwrap().iter());
        let second_end = bytes.len() - 1;

        fs::write(&path, &bytes).unwrap();
        assert_eq!(read_block(1).unwrap().transactions, vec![first.clone(), second]);
        for length in header_end..second_end {
            fs::write(&path, &bytes[..length]).unwrap();
            let expected = if length < first_end { vec![] } else { vec![first.clone()] };
            assert_eq!(read_block(1).unwrap().transactions, expected, "length {length}");
        }
    }

    #[test]
    fn create_replaces_the_file_left_for_the_block() {
        let _directory = journal_directory();
        fs::write(TransactionJournal::path(1.into()), b"left by a previous run").unwrap();

        let journal = TransactionJournal::create(header(1)).unwrap();
        journal.append(&JournalRecord::encode(&transaction(0)).unwrap()).unwrap();
        assert_eq!(read_block(1).unwrap().transactions, vec![transaction(0)]);
    }

    #[test]
    fn open_returns_error_when_the_journal_is_missing() {
        let _directory = journal_directory();
        assert!(TransactionJournal::open(1.into()).is_err());
    }

    #[test]
    fn journaled_blocks_opens_from_the_lowest_block_up_to_the_first_gap() {
        let _directory = journal_directory();
        for number in [3, 1, 2, 5] {
            TransactionJournal::create(header(number)).unwrap();
        }

        let mut journals = TransactionJournal::journaled_blocks().unwrap();
        for expected in [1, 2, 3] {
            let journal = journals.next().unwrap().unwrap();
            assert_eq!(journal.number(), BlockNumber::from(expected));
            journal.retire().unwrap();
        }
        assert!(journals.next().is_none());
    }

    #[test]
    fn journaled_blocks_yields_nothing_without_journals() {
        let _directory = journal_directory();
        assert!(TransactionJournal::journaled_blocks().unwrap().next().is_none());
    }

    #[test]
    fn dropping_the_iterator_sets_aside_the_journals_left() {
        let _directory = journal_directory();
        for number in [2, 3, 4, 6] {
            TransactionJournal::create(header(number)).unwrap();
        }
        let expected = fs::read(TransactionJournal::path(3.into())).unwrap();

        let mut journals = TransactionJournal::journaled_blocks().unwrap();
        journals.next().unwrap().unwrap().retire().unwrap();
        // a journal that fails to replay stays on disk for the drop to set aside
        journals.next().unwrap().unwrap();
        drop(journals);

        for number in [2, 3, 4, 6] {
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
            TransactionJournal::create(header(number)).unwrap();
        }

        drop(RollingJournal(TransactionJournal::open(2.into()).unwrap()));
        assert!(TransactionJournal::path(2.into()).exists());

        drop(RollingJournal(TransactionJournal::open(1.into()).unwrap()));
        assert!(!TransactionJournal::path(1.into()).exists());
    }
}
