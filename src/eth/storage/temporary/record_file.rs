//! Append-only file of newline-terminated records, written through a shared memory mapping.

use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::Read;
use std::path::Path;

use memmap2::MmapMut;

/// Size a file is preallocated to; files double in size when their records outgrow them
const INITIAL_CAPACITY: usize = 16 * 1024 * 1024;

#[derive(Debug)]
pub struct RecordFile {
    file: File,
    map: MmapMut,
    cursor: usize,
}

impl RecordFile {
    /// Creates the file at `path`, replacing the file left there
    pub fn create(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().create(true).truncate(true).read(true).write(true).open(path)?;
        file.set_len(INITIAL_CAPACITY as u64)?;
        let map = Self::map(&file)?;
        Ok(Self { file, map, cursor: 0 })
    }

    /// Opens the file at `path` to read its records
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let map = Self::map(&file)?;
        Ok(Self { file, map, cursor: 0 })
    }

    /// Appends a record followed by a newline
    pub fn append(&mut self, record: &[u8]) -> io::Result<()> {
        let record_end = self.cursor + record.len();
        if record_end + 1 > self.map.len() {
            self.grow(record_end + 1)?;
        }
        self.map[self.cursor..record_end].copy_from_slice(record);
        self.map[record_end] = b'\n';
        self.cursor = record_end + 1;
        Ok(())
    }

    fn grow(&mut self, required: usize) -> io::Result<()> {
        self.file.set_len(required.next_power_of_two() as u64)?;
        self.map = Self::map(&self.file)?;
        Ok(())
    }

    fn map(file: &File) -> io::Result<MmapMut> {
        // SAFETY: the file is resized and written only through its `RecordFile`, which holds the mapping
        unsafe { MmapMut::map_mut(file) }
    }
}

impl Read for RecordFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let unread = &self.map[self.cursor..];
        let read = unread.iter().take(buf.len()).take_while(|byte| **byte != 0).count();
        buf[..read].copy_from_slice(&unread[..read]);
        self.cursor += read;
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Read;
    use std::path::Path;
    use std::path::PathBuf;

    use super::INITIAL_CAPACITY;
    use super::RecordFile;

    fn path() -> PathBuf {
        tempfile::tempdir().unwrap().keep().join("records")
    }

    fn read_records(path: &Path) -> Vec<u8> {
        let mut records = Vec::new();
        RecordFile::open(path).unwrap().read_to_end(&mut records).unwrap();
        records
    }

    #[test]
    fn records_are_written_in_order() {
        let path = path();
        let mut file = RecordFile::create(&path).unwrap();
        file.append(b"first").unwrap();
        file.append(b"second").unwrap();
        drop(file);

        assert_eq!(read_records(&path), b"first\nsecond\n");
    }

    #[test]
    fn create_replaces_the_file_left_there() {
        let path = path();
        fs::write(&path, b"left by a previous run\n").unwrap();
        let mut file = RecordFile::create(&path).unwrap();
        file.append(b"first").unwrap();
        drop(file);

        assert_eq!(read_records(&path), b"first\n");
    }

    #[test]
    fn file_grows_past_initial_capacity() {
        let path = path();
        let mut file = RecordFile::create(&path).unwrap();
        let large = vec![0xab; INITIAL_CAPACITY];
        file.append(&large).unwrap();
        file.append(b"small").unwrap();
        drop(file);

        let expected = [large.as_slice(), b"\nsmall\n"].concat();
        assert_eq!(read_records(&path), expected);
    }
}
