use alloy_primitives::U256;
use display_json::DebugAsJson;
use fake::Dummy;
use fake::Faker;

use crate::gen_newtype_from;

#[derive(DebugAsJson, derive_more::Display, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Difficulty(pub U256);

impl Dummy<Faker> for Difficulty {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Difficulty, other = u8, u16, u32, u64, u128, U256, usize, i32);

impl From<[u64; 4]> for Difficulty {
    fn from(value: [u64; 4]) -> Self {
        Self(U256::from_limbs(value))
    }
}

impl From<U256> for Difficulty {
    fn from(value: U256) -> Self {
        Self(value)
    }
}
