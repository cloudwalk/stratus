use alloy_primitives::U256;
use display_json::DebugAsJson;
use fake::Dummy;
use fake::Faker;

// Type representing `r` and `s` variables from the ECDSA signature.
#[derive(DebugAsJson, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EcdsaRs(U256);

impl Dummy<Faker> for EcdsaRs {
    fn dummy_with_rng<R: rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self(U256::random_with(rng))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl From<u8> for EcdsaRs {
    fn from(value: u8) -> Self {
        Self(U256::from(value))
    }
}

impl From<u16> for EcdsaRs {
    fn from(value: u16) -> Self {
        Self(U256::from(value))
    }
}

impl From<u32> for EcdsaRs {
    fn from(value: u32) -> Self {
        Self(U256::from(value))
    }
}

impl From<u64> for EcdsaRs {
    fn from(value: u64) -> Self {
        Self(U256::from(value))
    }
}

impl From<u128> for EcdsaRs {
    fn from(value: u128) -> Self {
        Self(U256::from(value))
    }
}

impl From<U256> for EcdsaRs {
    fn from(value: U256) -> Self {
        Self(value)
    }
}

impl From<i8> for EcdsaRs {
    fn from(value: i8) -> Self {
        Self(U256::from(value as u8))
    }
}

impl From<i16> for EcdsaRs {
    fn from(value: i16) -> Self {
        Self(U256::from(value as u16))
    }
}

impl From<i32> for EcdsaRs {
    fn from(value: i32) -> Self {
        Self(U256::from(value as u32))
    }
}

impl From<i64> for EcdsaRs {
    fn from(value: i64) -> Self {
        Self(U256::from(value as u64))
    }
}

impl From<i128> for EcdsaRs {
    fn from(value: i128) -> Self {
        Self(U256::from(value as u128))
    }
}

impl From<[u8; 32]> for EcdsaRs {
    fn from(value: [u8; 32]) -> Self {
        Self(U256::from_be_bytes(value))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
