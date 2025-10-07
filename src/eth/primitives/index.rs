use alloy_primitives::U64;
use alloy_primitives::U256;
use anyhow::bail;
use derive_more::Deref;
use display_json::DebugAsJson;

use crate::ext::RuintExt;

/// Represents a transaction index or log index.
#[derive(
    DebugAsJson,
    derive_more::Display,
    Clone,
    Copy,
    PartialEq,
    Eq,
    fake::Dummy,
    serde::Serialize,
    serde::Deserialize,
    derive_more::Add,
    derive_more::AddAssign,
    Hash,
    PartialOrd,
    Ord,
    Deref,
    Default,
)]
pub struct Index(#[deref] pub u64);

impl Index {
    pub const ZERO: Index = Index(0u64);
    pub const ONE: Index = Index(1u64);

    pub fn new(inner: u64) -> Self {
        Index(inner)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl From<u64> for Index {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl TryFrom<U256> for Index {
    type Error = anyhow::Error;

    fn try_from(value: U256) -> Result<Self, Self::Error> {
        let u64_value = u64::try_from(value).map_err(|_| anyhow::anyhow!("U256 value too large for Index"))?;
        Ok(Self(u64_value))
    }
}

impl TryFrom<i64> for Index {
    type Error = anyhow::Error;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        if value < 0 {
            bail!("Index cannot be negative");
        }
        Ok(Self(value as u64))
    }
}

impl From<U64> for Index {
    fn from(value: U64) -> Self {
        Index(value.as_u64())
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<Index> for u64 {
    fn from(value: Index) -> u64 {
        value.0
    }
}

impl From<Index> for U64 {
    fn from(value: Index) -> U64 {
        U64::from(value.0)
    }
}

impl From<Index> for U256 {
    fn from(value: Index) -> U256 {
        U256::from(value.0)
    }
}
