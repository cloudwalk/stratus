use display_json::DebugAsJson;

use crate::eth::types::BlockNumber;
use crate::eth::types::UnixTimeNow;

/// Header of the pending block being mined.
#[derive(DebugAsJson, Clone, Copy, Default, serde::Serialize)]
pub struct PendingBlockHeader {
    pub number: BlockNumber,
    pub timestamp: UnixTimeNow,
}

impl PendingBlockHeader {
    /// Creates a new [`PendingBlockHeader`] with the specified number and the current timestamp.
    pub fn new_at_now(number: BlockNumber) -> Self {
        Self { number, ..Self::default() }
    }
}
