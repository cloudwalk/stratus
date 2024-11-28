use crate::eth::primitives::BlockNumber;
use crate::infra::metrics::MetricLabelValue;

/// EVM storage point-in-time indicator.
#[derive(Debug, strum::Display, Clone, Copy, Default, strum::EnumIs, serde::Serialize, serde::Deserialize, fake::Dummy, PartialEq)]
pub enum PointInTime {
    /// State of `Account` or `Slot` at the pending block being mined.
    ///
    /// If state did not change, then it is the same as the `Mined` state.
    #[strum(to_string = "pending")]
    Pending,

    /// State of `Account` or `Slot` at the last mined block.
    #[default]
    #[strum(to_string = "mined")]
    Mined,

    /// State of `Account` or `Slot` at some specific mined block in the past.
    #[strum(to_string = "mined-past")]
    MinedPast(BlockNumber),
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<PointInTime> for MetricLabelValue {
    fn from(value: PointInTime) -> Self {
        Self::Some(value.to_string())
    }
}
