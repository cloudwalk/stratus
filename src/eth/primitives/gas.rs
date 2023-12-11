use std::fmt::Display;

use ethereum_types::U256;

use crate::derive_newtype_from;

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(transparent)]
pub struct Gas(U256);

impl Display for Gas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
derive_newtype_from!(self = Gas, other = U256, u8, u16, u32, u64, u128, usize);

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// ----------------------------------------------------------------------------
impl From<Gas> for U256 {
    fn from(value: Gas) -> Self {
        value.0
    }
}
