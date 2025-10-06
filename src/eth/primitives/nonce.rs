use alloy_primitives::U64;
use alloy_primitives::U256;
use anyhow::bail;
use display_json::DebugAsJson;
#[cfg(test)]
use fake::Dummy;
#[cfg(test)]
use fake::Faker;

use crate::ext::RuintExt;

#[derive(DebugAsJson, derive_more::Display, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Nonce(U64);

impl Nonce {
    pub const ZERO: Nonce = Nonce(U64::ZERO);

    pub fn as_u64(&self) -> u64 {
        self.0.as_u64()
    }

    /// Returns the next nonce.
    pub fn next_nonce(&self) -> Self {
        Self(self.0 + U64::ONE)
    }
}

#[cfg(test)]
impl Dummy<Faker> for Nonce {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self(U64::random_with(rng))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl TryFrom<i32> for Nonce {
    type Error = anyhow::Error;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        if value < 0 {
            bail!("Nonce cannot be negative");
        }
        Ok(Self(U64::from(value as u32)))
    }
}

impl From<u64> for Nonce {
    fn from(value: u64) -> Self {
        Self(U64::from(value))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<Nonce> for u64 {
    fn from(value: Nonce) -> Self {
        value.as_u64()
    }
}

impl From<Nonce> for U256 {
    fn from(value: Nonce) -> Self {
        U256::from(value.as_u64())
    }
}
