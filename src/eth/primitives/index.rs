use std::num::TryFromIntError;

use ethereum_types::U256;
use ethereum_types::U64;

use crate::gen_newtype_from;
use crate::gen_newtype_try_from;

/// Represents a transaction index or log index.
#[derive(
    Debug, derive_more::Display, Clone, Copy, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize, derive_more::Add, Hash, PartialOrd, Ord,
)]
pub struct Index(u64);

impl Index {
    pub const ZERO: Index = Index(0u64);
    pub const ONE: Index = Index(1u64);

    pub fn new(inner: u64) -> Self {
        Index(inner)
    }

    pub fn inner_value(&self) -> u64 {
        self.0
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Index, other = u64);
gen_newtype_try_from!(self = Index, other = U256, i64);

impl From<U64> for Index {
    fn from(value: U64) -> Self {
        Index(value.as_u64())
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<Index> for U64 {
    fn from(value: Index) -> U64 {
        value.0.into()
    }
}

impl From<Index> for U256 {
    fn from(value: Index) -> U256 {
        value.0.into()
    }
}

impl TryFrom<Index> for i32 {
    type Error = TryFromIntError;

    fn try_from(value: Index) -> Result<Self, Self::Error> {
        value.0.try_into()
    }
}
