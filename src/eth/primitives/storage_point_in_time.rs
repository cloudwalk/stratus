//! Storage Point-in-Time Module
//!
//! Manages indicators for referencing the state of Ethereum storage at various
//! points in time. This is crucial for queries about past states, such as
//! determining the value of a variable at a specific block. The module provides
//! the ability to reference either the current state or a past state at a given
//! block number, facilitating temporal queries in Ethereum.

use crate::eth::primitives::BlockNumber;
#[cfg(feature = "metrics")]
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
#[cfg(feature = "metrics")]
impl From<&StoragePointInTime> for MetricLabelValue {
    fn from(value: &StoragePointInTime) -> Self {
        match value {
            StoragePointInTime::Present => Self::Some("present".to_string()),
            StoragePointInTime::Past(_) => Self::Some("past".to_string()),
        }
    }
}
