use alloy_primitives::U256;
use alloy_primitives::U64;
use display_json::DebugAsJson;

use crate::gen_newtype_from;
use crate::gen_newtype_try_from;

/// Represents a transaction index or log index.
#[derive(
    DebugAsJson, derive_more::Display, Clone, Copy, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize, derive_more::Add, Hash, PartialOrd, Ord,
)]
pub struct Index(pub u64);

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
gen_newtype_from!(self = Index, other = u64);
gen_newtype_try_from!(self = Index, other = U256, i64);

impl From<U64> for Index {
    fn from(value: U64) -> Self {
        Index(value.into_limbs()[0])
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
        value.0.try_into().expect("u64 fits into U64 qed")
    }
}

impl From<Index> for U256 {
    fn from(value: Index) -> U256 {
        value.0.try_into().expect("u64 fits into U256 qed")
    }
}
