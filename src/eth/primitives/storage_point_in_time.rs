use crate::eth::primitives::BlockNumber;
use crate::infra::metrics::MetricLabelValue;

/// EVM storage point-in-time indicator.
#[derive(Debug, strum::Display, Clone, Copy, Default, strum::EnumIs)]
pub enum StoragePointInTime {
    /// The current state of the EVM storage.
    #[default]
    #[strum(to_string = "present")]
    Present,

    /// The state of the EVM storage at the given block number.
    #[strum(to_string = "past")]
    Past(BlockNumber),
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<&StoragePointInTime> for MetricLabelValue {
    fn from(value: &StoragePointInTime) -> Self {
        match value {
            StoragePointInTime::Present => Self::Some("present".to_string()),
            StoragePointInTime::Past(_) => Self::Some("past".to_string()),
        }
    }
}
