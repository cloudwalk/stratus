use alloy_primitives::U64;
use anyhow::bail;
use display_json::DebugAsJson;
#[cfg(test)]
use fake::Dummy;
#[cfg(test)]
use fake::Faker;

use crate::ext::RuintExt;

#[derive(DebugAsJson, derive_more::Display, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Gas(U64);

impl Gas {
    pub const ZERO: Gas = Gas(U64::ZERO);
    pub const MAX: Gas = Gas(U64::MAX);

    pub fn as_u64(&self) -> u64 {
        self.0.as_u64()
    }
}

#[cfg(test)]
impl Dummy<Faker> for Gas {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl From<u8> for Gas {
    fn from(value: u8) -> Self {
        Self(U64::from(value))
    }
}

impl From<u16> for Gas {
    fn from(value: u16) -> Self {
        Self(U64::from(value))
    }
}

impl From<u32> for Gas {
    fn from(value: u32) -> Self {
        Self(U64::from(value))
    }
}

impl From<u64> for Gas {
    fn from(value: u64) -> Self {
        Self(U64::from(value))
    }
}

impl TryFrom<i32> for Gas {
    type Error = anyhow::Error;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        if value < 0 {
            bail!("Gas cannot be negative");
        }
        Ok(Self(U64::from(value as u32)))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// ----------------------------------------------------------------------------

impl From<Gas> for u64 {
    fn from(value: Gas) -> Self {
        value.as_u64()
    }
}
