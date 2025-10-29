use serde::Deserialize;
use serde::Serialize;

use crate::eth::primitives::UnixTime;
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Default, strum::Display, Hash)]
#[cfg_attr(test, derive(fake::Dummy))]
#[serde(rename_all = "camelCase")]
pub enum BlockTimestampSeekMode {
    #[default]
    ExactOrPrevious,
    ExactOrNext,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct BlockTimestampFilter {
    pub timestamp: UnixTime,
    #[serde(default)]
    pub mode: BlockTimestampSeekMode,
}

impl From<BlockTimestampSeekMode> for rocksdb::Direction {
    fn from(value: BlockTimestampSeekMode) -> Self {
        match value {
            BlockTimestampSeekMode::ExactOrNext => rocksdb::Direction::Forward,
            BlockTimestampSeekMode::ExactOrPrevious => rocksdb::Direction::Reverse,
        }
    }
}
