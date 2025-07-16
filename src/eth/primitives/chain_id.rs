use alloy_primitives::U64;
use alloy_primitives::U256;
use anyhow::anyhow;
use display_json::DebugAsJson;
use fake::Dummy;
use fake::Faker;


#[derive(DebugAsJson, derive_more::Display, Clone, Copy, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChainId(pub U64);

impl Dummy<Faker> for ChainId {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().try_into().expect("u64 fits into U64 qed.")
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl TryFrom<i32> for ChainId {
    type Error = anyhow::Error;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        if value < 0 {
            return Err(anyhow::anyhow!("ChainId cannot be negative"));
        }
        Ok(Self(U64::from(value as u32)))
    }
}


impl TryFrom<U256> for ChainId {
    type Error = anyhow::Error;

    fn try_from(value: U256) -> Result<Self, Self::Error> {
        Ok(ChainId(
            u64::try_from(value).map_err(|err| anyhow!(err))?.try_into().expect("u64 fits into U64 qed."),
        ))
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
        value.try_into().expect("U64 fits into u64 qed.")
    }
}

impl From<ChainId> for U256 {
    fn from(value: ChainId) -> Self {
        U256::from(u64::from(value))
    }
}
