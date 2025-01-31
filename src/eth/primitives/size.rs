use alloy_primitives::Uint;
use anyhow::anyhow;
use display_json::DebugAsJson;
use ethereum_types::U256;
use ethereum_types::U64;
use fake::Dummy;
use fake::Faker;

use crate::gen_newtype_from;

#[derive(DebugAsJson, derive_more::Display, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Size(U64);

impl Dummy<Faker> for Size {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Size, other = u8, u16, u32, u64);

impl TryFrom<U256> for Size {
    type Error = anyhow::Error;

    fn try_from(value: U256) -> Result<Self, Self::Error> {
        Ok(Size(u64::try_from(value).map_err(|err| anyhow!(err))?.into()))
    }
}
// TODO: improve before merging
impl TryFrom<alloy_primitives::Uint<256, 4>> for Size {
    type Error = anyhow::Error;

    fn try_from(value: alloy_primitives::Uint<256, 4>) -> Result<Self, Self::Error> {
        let as_u64 = u64::try_from(value).map_err(|_| anyhow!("Value too large for Size (must fit in u64)"))?;
        Ok(Size(as_u64.into()))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// ----------------------------------------------------------------------------
impl From<Size> for u64 {
    fn from(value: Size) -> Self {
        value.0.as_u64()
    }
}

impl From<Size> for Uint<256, 4> {
    fn from(value: Size) -> Self {
        Uint::from(value.0.as_u64())
    }
}
