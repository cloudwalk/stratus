use parking_lot::RwLock;
use parking_lot::RwLockReadGuard;
use parking_lot::RwLockWriteGuard;

use crate::eth::types::BlockInfo;

pub struct LatestStateLock(RwLock<BlockInfo>);
// could use ManuallyDrop instead
#[derive(Debug)]
pub struct LatestStateReadGuard<'a>(Option<RwLockReadGuard<'a, BlockInfo>>);

pub struct LatestStateWriteGuard<'a>(RwLockWriteGuard<'a, BlockInfo>);

impl<'a> LatestStateWriteGuard<'a> {
    pub fn set_latest_block_info(&mut self, block_info: BlockInfo) {
        (*self.0) = block_info;
    }
}

impl LatestStateLock {
    pub fn new(block_info: BlockInfo) -> Self {
        Self(RwLock::new(block_info))
    }

    pub fn read<'a>(&'a self) -> LatestStateReadGuard<'a> {
        LatestStateReadGuard(Some(self.0.read()))
    }

    pub fn write<'a>(&'a self) -> LatestStateWriteGuard<'a> {
        LatestStateWriteGuard(self.0.write())
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
