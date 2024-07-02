use crate::eth::primitives::BlockNumber;
use crate::infra::metrics::MetricLabelValue;

/// EVM storage point-in-time indicator.
#[derive(Debug, strum::Display, Clone, Copy, Default, strum::EnumIs)]
pub enum StoragePointInTime {
    /// State of the EVM temporary storage.
    Pending,

    /// State of the EVM permanent storage.
    #[default]
    #[strum(to_string = "present")]
    Present,

    /// State of the EVM permanent storage at the given block number.
    #[strum(to_string = "past")]
    Past(BlockNumber),
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<&StoragePointInTime> for MetricLabelValue {
    fn from(value: &StoragePointInTime) -> Self {
        Self::Some(value.to_string())
    }
}
