use crate::eth::primitives::BlockNumber;
use crate::infra::metrics::MetricLabelValue;

/// EVM storage point-in-time indicator.
#[derive(Debug, strum::Display, Clone, Copy, Default, strum::EnumIs)]
pub enum StoragePointInTime {
    /// State at EVM temporary storage.
    #[strum(to_string = "pending")]
    Temp,

    /// State at EVM permanent storage.
    #[default]
    #[strum(to_string = "mined")]
    Mined,

    /// State at EVM permanent storage at the given block number.
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
