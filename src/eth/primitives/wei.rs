use std::str::FromStr;

use display_json::DebugAsJson;
use ethereum_types::U256;
use fake::Dummy;
use fake::Faker;
use sqlx::types::BigDecimal;

use crate::alias::RevmU256;
use crate::gen_newtype_from;

/// Native token amount in wei.
#[derive(DebugAsJson, derive_more::Display, Clone, Copy, Default, PartialOrd, Ord, PartialEq, Eq, derive_more::Sub, serde::Serialize, serde::Deserialize)]
pub struct Wei(pub U256);

impl Wei {
    pub const ZERO: Wei = Wei(U256::zero());
    pub const ONE: Wei = Wei(U256::one());
    pub const TEST_BALANCE: Wei = Wei(U256([u64::MAX, 0, 0, 0]));

    pub fn as_u128(&self) -> u128 {
        self.0.as_u128()
    }
}

impl Dummy<Faker> for Wei {
    fn dummy_with_rng<R: rand_core::RngCore + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Wei, other = u8, u16, u32, u64, u128, U256, usize, i32);

impl From<[u64; 4]> for Wei {
    fn from(value: [u64; 4]) -> Self {
        Self(U256(value))
    }
}

impl From<RevmU256> for Wei {
    fn from(value: RevmU256) -> Self {
        Self(value.to_be_bytes().into())
    }
}

impl TryFrom<BigDecimal> for Wei {
    type Error = anyhow::Error;

    fn try_from(value: BigDecimal) -> Result<Self, Self::Error> {
        let value_str = value.to_string();
        Ok(Wei(U256::from_dec_str(&value_str)?))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<Wei> for u128 {
    fn from(value: Wei) -> Self {
        value.as_u128()
    }
}

impl From<Wei> for RevmU256 {
    fn from(value: Wei) -> Self {
        RevmU256::from_limbs(value.0 .0)
    }
}

impl From<Wei> for U256 {
    fn from(value: Wei) -> Self {
        value.0
    }
}

impl TryFrom<Wei> for BigDecimal {
    type Error = anyhow::Error;
    fn try_from(value: Wei) -> Result<Self, Self::Error> {
        // HACK: If we could import BigInt or BigUint we could convert the bytes directly.
        Ok(BigDecimal::from_str(&U256::from(value).to_string())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn big_decimal_to_nonce_conversion() {
        // Test with a simple value
        let big_decimal = BigDecimal::new(1.into(), -4);
        let nonce: Wei = big_decimal.clone().try_into().unwrap();
        let expected = nonce.0.as_u64();
        assert_eq!(10000, expected);
    }
}
