use crate::eth::primitives::BlockNumber;
use crate::infra::metrics::LabelValue;

/// EVM storage point-in-time indicator.
#[derive(Debug)]
pub enum StoragerPointInTime {
    /// The current state of the EVM storage.
    Present,

    /// The state of the EVM storage at the given block number.
    Past(BlockNumber),
}

impl From<&StoragerPointInTime> for LabelValue {
    fn from(value: &StoragerPointInTime) -> Self {
        match value {
            StoragerPointInTime::Present => Self::Some("present".to_string()),
            StoragerPointInTime::Past(_) => Self::Some("past".to_string()),
        }
    }
}
