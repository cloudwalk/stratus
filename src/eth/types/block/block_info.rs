use display_json::DebugAsJson;

use crate::eth::types::BlockHeader;
use crate::eth::types::BlockNumber;
use crate::eth::types::UnixTimeNow;

/// Block information used on evm executions
#[derive(DebugAsJson, Clone, Copy, Default, serde::Serialize)]
pub struct BlockInfo {
    pub number: BlockNumber,
    pub timestamp: UnixTimeNow,
}

impl BlockInfo {
    /// Creates a new [`BlockInfo`] with the specified number and the current timestamp.
    pub fn new_at_now(number: BlockNumber) -> Self {
        Self { number, ..Self::default() }
    }
}

impl From<BlockHeader> for BlockInfo {
    fn from(value: BlockHeader) -> Self {
        (&value).into()
    }
}

impl From<&BlockHeader> for BlockInfo {
    fn from(value: &BlockHeader) -> Self {
        Self {
            number: value.number,
            timestamp: value.timestamp.into(),
        }
    }
}
