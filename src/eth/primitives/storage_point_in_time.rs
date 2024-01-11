use crate::eth::primitives::BlockNumber;
use crate::infra::metrics::LabelValue;

/// EVM storage point-in-time indicator.
#[derive(Debug)]
pub enum StoragePointInTime {
    /// The current state of the EVM storage.
    Present,

    /// The state of the EVM storage at the given block number.
    Past(BlockNumber),
}

impl From<&StoragePointInTime> for LabelValue {
    fn from(value: &StoragePointInTime) -> Self {
        match value {
            StoragePointInTime::Present => Self::Some("present".to_string()),
            StoragePointInTime::Past(_) => Self::Some("past".to_string()),
        }
    }
}
