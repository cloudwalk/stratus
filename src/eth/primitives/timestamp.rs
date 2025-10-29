use serde::Deserialize;
use serde::Serialize;

use crate::eth::primitives::UnixTime;
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Default, strum::Display)]
pub enum BlockTimestampSeekMode {
    #[default]
    ExactOrPrevious,
    ExactOrNext,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockTimestampSeek {
    pub timestamp: UnixTime,
    #[serde(default)]
    pub mode: BlockTimestampSeekMode,
}
