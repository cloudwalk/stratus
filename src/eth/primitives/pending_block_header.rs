use display_json::DebugAsJson;

use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::UnixTimeNow;

/// Header of the pending block being mined.
#[derive(DebugAsJson, Clone, Default, serde::Serialize)]
pub struct PendingBlockHeader {
    pub number: BlockNumber,
    pub timestamp: UnixTimeNow,
    pub parent_hash: Option<Hash>,
}

impl PendingBlockHeader {
    /// Creates a new [`PendingBlockHeader`] with the specified number and the current timestamp.
    pub fn new_at_now(number: BlockNumber) -> Self {
        Self { number, ..Self::default() }
    }
}
