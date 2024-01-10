use std::fmt::Display;

use ethereum_types::U256;
use fake::Dummy;
use fake::Faker;

use crate::gen_newtype_from;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Gas(U256);

impl Gas {
    pub const ZERO: Gas = Gas(U256::zero());
}

impl Display for Gas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Dummy<Faker> for Gas {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Gas, other = u8, u16, u32, u64, u128, U256, usize, i32);

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// ----------------------------------------------------------------------------
impl From<Gas> for U256 {
    fn from(value: Gas) -> Self {
        value.0
    }
}

impl From<Gas> for usize {
    fn from(value: Gas) -> Self {
        value.0.as_usize()
    }
}
