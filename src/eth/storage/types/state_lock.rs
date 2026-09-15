use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use parking_lot::RwLock;
use parking_lot::RwLockReadGuard;
use parking_lot::RwLockWriteGuard;

use crate::eth::types::BlockInfo;
use crate::eth::types::BlockNumber;
use crate::eth::types::UnixTime;

pub struct LatestStateLock {
    state: RwLock<BlockInfo>,
    /// Latest block number, mirrored for relaxed readers.
    /// We can store both in one atomic by casting number and timestamp to u32.
    /// Blocknumbers are nowhere close 4b and timestamp is good till 2107.
    /// Would allow for gains for all call executions. Right now the only reason
    /// we can't read directly from the atomics even though the atomic writes are being serialized
    /// is the extreme tearing edge case where a reader reads the number after it has been updated but the
    /// timestamp before it has been updated.
    number: AtomicU64,
    /// Latest block timestamp, mirrored for relaxed readers.
    timestamp: AtomicU64,
}
// could use ManuallyDrop instead
#[derive(Debug)]
pub struct LatestStateReadGuard<'a>(Option<RwLockReadGuard<'a, BlockInfo>>);

pub struct LatestStateWriteGuard<'a> {
    state: RwLockWriteGuard<'a, BlockInfo>,
    number: &'a AtomicU64,
    timestamp: &'a AtomicU64,
}

impl<'a> LatestStateWriteGuard<'a> {
    pub fn set_latest_block_info(&mut self, block_info: BlockInfo) {
        let number = block_info.number.as_u64();
        let timestamp = **block_info.timestamp;
        (*self.state) = block_info;

        self.number.store(number, Ordering::Relaxed);
        self.timestamp.store(timestamp, Ordering::Relaxed);
    }
}

impl LatestStateLock {
    pub fn new(block_info: BlockInfo) -> Self {
        let number = block_info.number.as_u64();
        let timestamp = **block_info.timestamp;
        Self {
            state: RwLock::new(block_info),
            number: AtomicU64::new(number),
            timestamp: AtomicU64::new(timestamp),
        }
    }

    pub fn read<'a>(&'a self) -> LatestStateReadGuard<'a> {
        LatestStateReadGuard(Some(self.state.read()))
    }

    pub fn write<'a>(&'a self) -> LatestStateWriteGuard<'a> {
        LatestStateWriteGuard {
            state: self.state.write(),
            number: &self.number,
            timestamp: &self.timestamp,
        }
    }

    /// Reads the latest block info from the atomic mirrors, without acquiring the state lock.
    pub fn read_latest_block_info_relaxed(&self) -> BlockInfo {
        let number = self.number.load(Ordering::Relaxed);
        let timestamp = self.timestamp.load(Ordering::Relaxed);
        BlockInfo {
            number: BlockNumber::from(number),
            timestamp: UnixTime::from(timestamp).into(),
        }
    }
}

impl std::ops::Deref for LatestStateReadGuard<'_> {
    type Target = BlockInfo;
    fn deref(&self) -> &BlockInfo {
        #[allow(clippy::expect_used)]
        self.0.as_ref().expect("guard present until dropped")
    }
}

impl Drop for LatestStateReadGuard<'_> {
    fn drop(&mut self) {
        if let Some(guard) = self.0.take() {
            RwLockReadGuard::unlock_fair(guard);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eth::types::UnixTime;

    fn block_info(number: u64, timestamp: u64) -> BlockInfo {
        BlockInfo {
            number: BlockNumber::from(number),
            timestamp: UnixTime::from(timestamp).into(),
        }
    }

    #[test]
    fn mirrors_latest_block_info_into_relaxed_reads() {
        let lock = LatestStateLock::new(block_info(42, 1_700_000_000));
        assert_eq!(lock.read_latest_block_info_relaxed().number.as_u64(), 42);

        let mut guard = lock.write();
        guard.set_latest_block_info(block_info(43, 1_700_000_001));
        drop(guard);

        let relaxed = lock.read_latest_block_info_relaxed();
        assert_eq!(relaxed.number.as_u64(), 43);
        assert_eq!(*relaxed.timestamp, UnixTime::from(1_700_000_001));
    }
}
