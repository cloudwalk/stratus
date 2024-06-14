use anyhow::anyhow;
use ethereum_types::U256;
use ethereum_types::U64;
use fake::Dummy;
use fake::Faker;

use crate::gen_newtype_from;
use crate::gen_newtype_try_from;

#[derive(Debug, derive_more::Display, Clone, Copy, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChainId(U64);

impl ChainId {
    pub fn new(value: U64) -> Self {
        Self(value)
    }

    pub fn inner_value(&self) -> U64 {
        self.0
    }
}

impl Dummy<Faker> for ChainId {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = ChainId, other = u8, u16, u32, u64);
gen_newtype_try_from!(self = ChainId, other = i32);

impl TryFrom<U256> for ChainId {
    type Error = anyhow::Error;

    fn try_from(value: U256) -> Result<Self, Self::Error> {
        Ok(ChainId(u64::try_from(value).map_err(|err| anyhow!(err))?.into()))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<ChainId> for u64 {
    fn from(value: ChainId) -> Self {
        value.0.as_u64()
    }
}

impl From<ChainId> for U256 {
    fn from(value: ChainId) -> Self {
        value.0.as_u64().into()
    }
}
