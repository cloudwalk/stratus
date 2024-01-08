use itertools::Itertools;
use revm::primitives::Log as RevmLog;

use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::LogTopic;

/// Log is an event emitted by the EVM during contract execution.
#[derive(Debug, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct Log {
    /// Address that emitted the log.
    pub address: Address,

    /// Topics (0 to 4 positions) describing the log.
    pub topics: Vec<LogTopic>,

    /// Additional data.
    pub data: Bytes,
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// ----------------------------------------------------------------------------
impl From<RevmLog> for Log {
    fn from(value: RevmLog) -> Self {
        Self {
            address: value.address.into(),
            topics: value.topics.into_iter().map_into().collect(),
            data: value.data.into(),
        }
    }
}
