use alloy_primitives::U64;
use display_json::DebugAsJson;
use fake::Dummy;
use fake::Faker;

use crate::gen_newtype_from;
use crate::gen_newtype_try_from;

#[derive(DebugAsJson, derive_more::Display, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Gas(U64);

impl Gas {
    pub const ZERO: Gas = Gas(U64::ZERO);
    pub const MAX: Gas = Gas(U64::MAX);

    pub fn as_u64(&self) -> u64 {
        self.0.try_into().expect("U64 fits into u64 qed.")
    }
}

impl Dummy<Faker> for Gas {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Gas, other = u8, u16, u32, u64);
gen_newtype_try_from!(self = Gas, other = i32);

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// ----------------------------------------------------------------------------

impl From<Gas> for u64 {
    fn from(value: Gas) -> Self {
        value.as_u64()
    }
}
