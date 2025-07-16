use std::fmt::Display;

use alloy_primitives::U256;
use display_json::DebugAsJson;
use fake::Dummy;
use fake::Faker;

use crate::gen_newtype_from;

#[derive(DebugAsJson, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SlotValue(pub U256);

impl SlotValue {
    /// Converts itself to [`U256`].
    pub fn as_u256(&self) -> U256 {
        self.0
    }
}

impl Display for SlotValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#x}", self.0)
    }
}

impl Dummy<Faker> for SlotValue {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self(U256::random_with(rng))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<SlotValue> for U256 {
    fn from(value: SlotValue) -> Self {
        value.0
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = SlotValue, other = U256);


impl From<[u64; 4]> for SlotValue {
    fn from(value: [u64; 4]) -> Self {
        Self(U256::from_limbs(value))
    }
}



impl From<alloy_primitives::FixedBytes<32>> for SlotValue {
    fn from(value: alloy_primitives::FixedBytes<32>) -> Self {
        Self::from(U256::from_be_bytes(value.0))
    }
}
