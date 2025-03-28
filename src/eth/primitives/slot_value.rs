use std::fmt::Display;

use display_json::DebugAsJson;
use ethereum_types::U256;
use fake::Dummy;
use fake::Faker;

use crate::alias::RevmU256;
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
    fn dummy_with_rng<R: rand_core::RngCore + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

gen_newtype_from!(self = SlotValue, other = u64, U256, [u8; 32]);

impl From<SlotValue> for RevmU256 {
    fn from(value: SlotValue) -> Self {
        RevmU256::from_limbs(value.0 .0)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl From<RevmU256> for SlotValue {
    fn from(value: RevmU256) -> Self {
        Self(value.to_be_bytes().into())
    }
}

impl From<[u64; 4]> for SlotValue {
    fn from(value: [u64; 4]) -> Self {
        Self(U256(value))
    }
}

impl From<alloy_primitives::FixedBytes<32>> for SlotValue {
    fn from(value: alloy_primitives::FixedBytes<32>) -> Self {
        Self::from(value.0)
    }
}
