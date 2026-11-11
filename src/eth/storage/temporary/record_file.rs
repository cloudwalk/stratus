//! Append-only file of newline-terminated records, written through a shared memory mapping.

use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::path::Path;

use memmap2::MmapMut;

/// Size a file is preallocated to; files double in size when their records outgrow them.
const INITIAL_CAPACITY: usize = 64 * 1024 * 1024;

/// Appending copies bytes into the mapping, which lands them in the kernel page cache, so records outlive a crash
/// of the process.
///
/// The preallocated space is zeroed, so the records end at the first zero byte. A crash during an append leaves a
/// record missing its newline or holding a zero byte, which reads as the end of the records.
#[derive(Debug)]
pub struct RecordFile {
    file: File,
    map: MmapMut,
    /// Offset where the next record starts.
    end: usize,
}

impl RecordFile {
    /// Opens the file at `path`, creating it when missing and positioning appends after the records it holds.
    ///
    /// Bytes past the last complete record, left by a crash during an append, are discarded.
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(path)?;
        let len = file.metadata()?.len();
        let end = if len == 0 { 0 } else { read_records(&Self::map(&file)?).1 };

        // truncating to the last record and extending again zeroes everything after it
        file.set_len(end as u64)?;
        file.set_len(len.max(INITIAL_CAPACITY as u64))?;

        let map = Self::map(&file)?;
        Ok(Self { file, map, end })
    }

    pub fn is_empty(&self) -> bool {
        self.end == 0
    }

    /// Returns the records, without their newlines.
    pub fn records(&self) -> Vec<&[u8]> {
        read_records(&self.map[..self.end]).0
    }

    /// Appends a record, which must hold no newline or zero byte.
    pub fn append(&mut self, record: &[u8]) -> io::Result<()> {
        debug_assert!(!record.contains(&b'\n') && !record.contains(&0), "record holds a newline or zero byte");
        let record_end = self.end + record.len();
        if record_end + 1 > self.map.len() {
            self.grow(record_end + 1)?;
        }
        self.map[self.end..record_end].copy_from_slice(record);
        self.map[record_end] = b'\n';
        self.end = record_end + 1;
        Ok(())
    }

    fn grow(&mut self, required: usize) -> io::Result<()> {
        self.file.set_len(required.next_power_of_two() as u64)?;
        self.map = Self::map(&self.file)?;
        Ok(())
    }

    fn map(file: &File) -> io::Result<MmapMut> {
        // SAFETY: the file is resized and written only through its `RecordFile`, which holds the mapping.
        unsafe { MmapMut::map_mut(file) }
    }
}

/// Splits bytes into the complete records before the first zero byte, returning them with the offset after the
/// last one.
fn read_records(bytes: &[u8]) -> (Vec<&[u8]>, usize) {
    let written = bytes.iter().position(|byte| *byte == 0).map_or(bytes, |zero| &bytes[..zero]);
    let end = written.iter().rposition(|byte| *byte == b'\n').map_or(0, |newline| newline + 1);
    if end < written.len() {
        tracing::error!(offset = end, "record is incomplete, ignoring as to not fail startup");
    }
    let records = written[..end].split(|byte| *byte == b'\n').filter(|record| !record.is_empty()).collect();
    (records, end)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::INITIAL_CAPACITY;
    use super::RecordFile;
    use super::read_records;

    fn path() -> PathBuf {
        tempfile::tempdir().unwrap().keep().join("records")
    }

    #[test]
    fn truncation_preserves_complete_records_in_order() {
        let records: [&[u8]; 3] = [b"header", b"first", b"second"];
        let mut bytes = Vec::new();
        let mut ends = Vec::new();
        for record in records {
            bytes.extend_from_slice(record);
            bytes.push(b'\n');
            ends.push(bytes.len());
        }

        for length in 0..=bytes.len() {
            let complete = ends.iter().filter(|end| **end <= length).count();
            assert_eq!(read_records(&bytes[..length]).0, records[..complete], "length {length}");
        }
    }

    #[test]
    fn record_with_unwritten_byte_ends_the_records() {
        // a crash during the copy can store the newline before an earlier byte of the record
        assert_eq!(read_records(b"first\nsec\0nd\n").0, [b"first"]);
    }

    #[test]
    fn records_survive_reopen_in_order() {
        let path = path();
        let mut file = RecordFile::open(&path).unwrap();
        assert!(file.is_empty());
        file.append(b"first").unwrap();
        file.append(b"second").unwrap();
        drop(file);

        let mut file = RecordFile::open(&path).unwrap();
        file.append(b"third").unwrap();
        assert_eq!(file.records(), [b"first".as_slice(), b"second", b"third"]);
    }

    #[test]
    fn record_cut_short_is_discarded_on_resume() {
        let path = path();
        let mut file = RecordFile::open(&path).unwrap();
        file.append(b"first").unwrap();

        // bytes of a record cut short by a crash, scattered past an unwritten byte
        let end = file.end;
        file.map[end..end + 3].copy_from_slice(b"sec");
        file.map[end + 4..end + 8].copy_from_slice(b"ond\n");
        drop(file);

        let mut file = RecordFile::open(&path).unwrap();
        assert_eq!(file.records(), [b"first"]);
        file.append(b"2").unwrap();
        drop(file);

        assert_eq!(RecordFile::open(&path).unwrap().records(), [b"first".as_slice(), b"2"]);
    }

    #[test]
    fn file_grows_past_initial_capacity() {
        let path = path();
        let mut file = RecordFile::open(&path).unwrap();
        let large = vec![0xab; INITIAL_CAPACITY];
        file.append(&large).unwrap();
        file.append(b"small").unwrap();
        drop(file);

        assert_eq!(RecordFile::open(&path).unwrap().records(), [large.as_slice(), b"small"]);
    }
}
