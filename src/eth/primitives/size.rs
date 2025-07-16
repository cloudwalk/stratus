use alloy_primitives::U256;
use alloy_primitives::U64;
use anyhow::anyhow;
use display_json::DebugAsJson;
use fake::Dummy;
use fake::Faker;


#[derive(DebugAsJson, derive_more::Display, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Size(U64);

impl Dummy<Faker> for Size {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self(U64::random_with(rng))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl From<u64> for Size {
    fn from(value: u64) -> Self {
        Self(U64::from(value))
    }
}

impl TryFrom<U256> for Size {
    type Error = anyhow::Error;

    fn try_from(value: U256) -> Result<Self, Self::Error> {
        Ok(Size(U64::from(u64::try_from(value).map_err(|err| anyhow!(err))?)))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// ----------------------------------------------------------------------------
impl From<Size> for u64 {
    fn from(value: Size) -> Self {
        value.0.as_limbs()[0]
    }
}

impl From<Size> for U256 {
    fn from(value: Size) -> Self {
        U256::from(value.0)
    }
}
