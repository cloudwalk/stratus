use alloy_primitives::U64;
use alloy_primitives::U256;
use anyhow::anyhow;
use anyhow::bail;
use display_json::DebugAsJson;
#[cfg(test)]
use fake::Dummy;
#[cfg(test)]
use fake::Faker;

use crate::ext::RuintExt;

#[derive(DebugAsJson, derive_more::Display, Clone, Copy, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChainId(pub U64);

#[cfg(test)]
impl Dummy<Faker> for ChainId {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl TryFrom<i32> for ChainId {
    type Error = anyhow::Error;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        if value < 0 {
            bail!("ChainId cannot be negative");
        }
        Ok(Self(U64::from(value as u32)))
    }
}

impl TryFrom<U256> for ChainId {
    type Error = anyhow::Error;

    fn try_from(value: U256) -> Result<Self, Self::Error> {
        Ok(ChainId(U64::from(u64::try_from(value).map_err(|err| anyhow!(err))?)))
    }
}

impl From<u64> for ChainId {
    fn from(value: u64) -> Self {
        Self(U64::from(value))
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
        U256::from(u64::from(value))
    }
}
