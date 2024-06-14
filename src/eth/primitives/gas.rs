use anyhow::anyhow;
use ethereum_types::U256;
use ethereum_types::U64;
use fake::Dummy;
use fake::Faker;

use crate::gen_newtype_from;
use crate::gen_newtype_try_from;

#[derive(Debug, derive_more::Display, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Gas(U64);

impl Gas {
    pub const ZERO: Gas = Gas(U64::zero());
    pub const MAX: Gas = Gas(U64::MAX);

    pub fn as_u64(&self) -> u64 {
        self.0.as_u64()
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
gen_newtype_from!(self = Gas, other = u8, u16, u32, u64);
gen_newtype_try_from!(self = Gas, other = i32);

impl TryFrom<U256> for Gas {
    type Error = anyhow::Error;

    fn try_from(value: U256) -> Result<Self, Self::Error> {
        Ok(Gas(u64::try_from(value).map_err(|err| anyhow!(err))?.into()))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// ----------------------------------------------------------------------------
impl From<Gas> for U256 {
    fn from(value: Gas) -> Self {
        value.0.as_u64().into()
    }
}

impl From<Gas> for u64 {
    fn from(value: Gas) -> Self {
        value.0.as_u64()
    }
}
