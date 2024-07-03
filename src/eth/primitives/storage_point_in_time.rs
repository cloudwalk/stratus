use crate::eth::primitives::BlockNumber;
use crate::infra::metrics::MetricLabelValue;

/// EVM storage point-in-time indicator.
#[derive(Debug, strum::Display, Clone, Copy, Default, strum::EnumIs)]
pub enum StoragePointInTime {
    /// Current state of temporary storage.
    #[strum(to_string = "temp")]
    Temporary,

    /// Current state of permanent storage.
    #[default]
    #[strum(to_string = "mined")]
    Mined,

    /// Past state of permanent storage at the given block number.
    #[strum(to_string = "mined-past")]
    MinedPast(BlockNumber),
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<&StoragePointInTime> for MetricLabelValue {
    fn from(value: &StoragePointInTime) -> Self {
        Self::Some(value.to_string())
    }
}
