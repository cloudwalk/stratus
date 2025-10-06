use alloy_primitives::U256;
use display_json::DebugAsJson;
#[cfg(test)]
use fake::Dummy;
#[cfg(test)]
use fake::Faker;

#[derive(DebugAsJson, derive_more::Display, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Difficulty(pub U256);

#[cfg(test)]
impl Dummy<Faker> for Difficulty {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl From<u8> for Difficulty {
    fn from(value: u8) -> Self {
        Self(U256::from(value))
    }
}

impl From<u16> for Difficulty {
    fn from(value: u16) -> Self {
        Self(U256::from(value))
    }
}

impl From<u32> for Difficulty {
    fn from(value: u32) -> Self {
        Self(U256::from(value))
    }
}

impl From<u64> for Difficulty {
    fn from(value: u64) -> Self {
        Self(U256::from(value))
    }
}

impl From<u128> for Difficulty {
    fn from(value: u128) -> Self {
        Self(U256::from(value))
    }
}

impl From<U256> for Difficulty {
    fn from(value: U256) -> Self {
        Self(value)
    }
}

impl From<usize> for Difficulty {
    fn from(value: usize) -> Self {
        Self(U256::from(value))
    }
}

impl From<i32> for Difficulty {
    fn from(value: i32) -> Self {
        Self(U256::from(value as u32))
    }
}

impl From<[u64; 4]> for Difficulty {
    fn from(value: [u64; 4]) -> Self {
        Self(U256::from_limbs(value))
    }
}
