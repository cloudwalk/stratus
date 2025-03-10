use display_json::DebugAsJson;
use ethereum_types::U256;
use fake::Dummy;
use fake::Faker;

use crate::alias::AlloyUint256;
use crate::gen_newtype_from;

#[derive(DebugAsJson, derive_more::Display, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Difficulty(pub U256);

impl Dummy<Faker> for Difficulty {
    fn dummy_with_rng<R: rand_core::RngCore + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Difficulty, other = u8, u16, u32, u64, u128, U256, usize, i32, [u8; 32]);

impl From<[u64; 4]> for Difficulty {
    fn from(value: [u64; 4]) -> Self {
        Self(U256(value))
    }
}

impl From<AlloyUint256> for Difficulty {
    fn from(value: AlloyUint256) -> Self {
        Self(U256::from_big_endian(&value.to_be_bytes::<32>()))
    }
}
