use display_json::DebugAsJson;

use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::UnixTime;

#[derive(DebugAsJson, Clone, Default, serde::Serialize)]
pub struct PendingBlockHeader {
    pub number: BlockNumber,
    pub timestamp: UnixTime,
}

impl PendingBlockHeader {
    /// Creates a new [`PendingBlockHeader`] with the specified number and the current timestamp.
    pub fn new_at_now(number: BlockNumber) -> Self {
        Self {
            number,
            timestamp: UnixTime::now(),
        }
    }
}
