//! Journal of the pending block's transactions, persisted so a restart can replay the blocks that never reached
//! permanent storage.

pub use transaction_journal::JournalBlock;
pub use transaction_journal::JournalRecord;
pub use transaction_journal::RollingJournal;
pub use transaction_journal::TransactionJournal;

mod record_file;
mod transaction_journal;

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Read;
    use std::path::Path;
    use std::path::PathBuf;

    use parking_lot::Mutex;
    use parking_lot::MutexGuard;

    use super::JournalRecord;
    use super::RollingJournal;
    use super::TransactionJournal;
    use super::record_file::INITIAL_CAPACITY;
    use super::record_file::RecordFile;
    use super::transaction_journal;
    use crate::eth::types::BlockInfo;
    use crate::eth::types::BlockNumber;
    use crate::eth::types::TransactionInput;
    use crate::eth::types::UnixTimeNow;
    use crate::globals::DATABASE_PATH;

    fn record_path() -> PathBuf {
        tempfile::tempdir().unwrap().keep().join("records")
    }

    fn read_records(path: &Path) -> Vec<u8> {
        let mut records = Vec::new();
        RecordFile::open(path).unwrap().read_to_end(&mut records).unwrap();
        records
    }

    #[test]
    fn records_are_written_in_order() {
        let path = record_path();
        let mut file = RecordFile::create(&path).unwrap();
        file.append(b"first").unwrap();
        file.append(b"second").unwrap();
        drop(file);

        assert_eq!(read_records(&path), b"first\nsecond\n");
    }

    #[test]
    fn create_replaces_the_file_left_there() {
        let path = record_path();
        fs::write(&path, b"left by a previous run\n").unwrap();
        let mut file = RecordFile::create(&path).unwrap();
        file.append(b"first").unwrap();
        drop(file);

        assert_eq!(read_records(&path), b"first\n");
    }

    #[test]
    fn file_grows_past_initial_capacity() {
        let path = record_path();
        let mut file = RecordFile::create(&path).unwrap();
        let large = vec![0xab; INITIAL_CAPACITY];
        file.append(&large).unwrap();
        file.append(b"small").unwrap();
        drop(file);

        let expected = [large.as_slice(), b"\nsmall\n"].concat();
        assert_eq!(read_records(&path), expected);
    }
    /// Serializes the tests sharing the journal directory: dropping a `JournalIter` moves every journal in it.
    static JOURNAL_DIRECTORY: Mutex<()> = Mutex::new(());

    /// Anchors the database path to a temporary directory and takes the journal directory, emptied, for the test.
    fn journal_directory() -> MutexGuard<'static, ()> {
        let guard = JOURNAL_DIRECTORY.lock();
        let _ = DATABASE_PATH.set(tempfile::tempdir().unwrap().keep());
        let _ = fs::remove_dir_all(transaction_journal::journal_directory());
        fs::create_dir_all(transaction_journal::journal_directory()).unwrap();
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
    fn journaled_blocks_opens_from_the_lowest_block_up_to_the_first_gap() {
        let _directory = journal_directory();
        for number in [3, 1, 2, 5] {
            TransactionJournal::create(header(number, 123)).unwrap();
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
    fn dropping_the_iterator_sets_aside_the_journals_left() {
        let _directory = journal_directory();
        for number in [2, 3, 4, 6] {
            TransactionJournal::create(header(number, 123)).unwrap();
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
        let unrecovered = fs::read_dir(transaction_journal::journal_directory().join(transaction_journal::UNRECOVERED_DIRECTORY))
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
