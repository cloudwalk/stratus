use alloy_primitives::U64;
use display_json::DebugAsJson;
use fake::Dummy;
use fake::Faker;


// Type representing `v` variable from the ECDSA signature.
#[derive(DebugAsJson, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EcdsaV(U64);

impl Dummy<Faker> for EcdsaV {
    fn dummy_with_rng<R: rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        Self::from(rng.next_u64())
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl From<u8> for EcdsaV {
    fn from(value: u8) -> Self {
        Self(U64::from(value))
    }
}

impl From<u16> for EcdsaV {
    fn from(value: u16) -> Self {
        Self(U64::from(value))
    }
}

impl From<u32> for EcdsaV {
    fn from(value: u32) -> Self {
        Self(U64::from(value))
    }
}

impl From<u64> for EcdsaV {
    fn from(value: u64) -> Self {
        Self(U64::from(value))
    }
}

impl From<U64> for EcdsaV {
    fn from(value: U64) -> Self {
        Self(value)
    }
}

impl From<i8> for EcdsaV {
    fn from(value: i8) -> Self {
        Self(U64::from(value as u8))
    }
}

impl From<i16> for EcdsaV {
    fn from(value: i16) -> Self {
        Self(U64::from(value as u16))
    }
}

impl From<i32> for EcdsaV {
    fn from(value: i32) -> Self {
        Self(U64::from(value as u32))
    }
}

impl From<[u8; 8]> for EcdsaV {
    fn from(value: [u8; 8]) -> Self {
        Self(U64::from_be_bytes(value))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
