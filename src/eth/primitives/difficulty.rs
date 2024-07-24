use ethereum_types::U256;
use fake::Dummy;
use fake::Faker;

use crate::gen_newtype_from;

#[derive(Debug, derive_more::Display, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Difficulty(pub U256);

impl Dummy<Faker> for Difficulty {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
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

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// ----------------------------------------------------------------------------
impl From<Difficulty> for U256 {
    fn from(value: Difficulty) -> Self {
        value.0
    }
}

impl From<Difficulty> for u64 {
    fn from(value: Difficulty) -> Self {
        value.0.low_u64()
    }
}
