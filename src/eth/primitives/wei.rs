//! Wei Module
//!
//! Manages Wei, the smallest denomination of Ether, Ethereum's native
//! cryptocurrency. Wei is essential for representing transaction values,
//! calculating gas costs, and handling financial operations on the Ethereum
//! network. This module includes functionalities for manipulating Wei values,
//! enabling precise financial operations and conversions within the Ethereum
//! ecosystem.

use std::fmt::Display;
use std::str::FromStr;

use ethabi::Token;
use ethereum_types::U256;
use fake::Dummy;
use fake::Faker;
use revm::primitives::U256 as RevmU256;
use sqlx::database::HasValueRef;
use sqlx::error::BoxDynError;
use sqlx::types::BigDecimal;
use sqlx::Decode;

use crate::gen_newtype_from;

/// Native token amount in wei.
#[derive(Debug, Clone, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Wei(U256);

impl Wei {
    pub const ZERO: Wei = Wei(U256::zero());
    pub const ONE: Wei = Wei(U256::one());
    pub const TEST_BALANCE: Wei = Wei(U256([u64::MAX, 0, 0, 0]));
}

impl Display for Wei {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Dummy<Faker> for Wei {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Wei, other = u8, u16, u32, u64, u128, U256, usize, i32);

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
// Conversions: sqlx -> Self
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for Wei {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <BigDecimal as Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.try_into()?)
    }
}

impl sqlx::Type<sqlx::Postgres> for Wei {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("NUMERIC")
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<Wei> for Token {
    fn from(value: Wei) -> Self {
        Token::Uint(value.0)
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
    use sqlx::types::BigDecimal;

    use super::*; // Adjust this as necessary to bring your types into scope

    #[test]
    fn big_decimal_to_nonce_conversion() {
        // Test with a simple value
        let big_decimal = BigDecimal::new(1.into(), -4);
        let nonce: Wei = big_decimal.clone().try_into().unwrap();
        let expected = nonce.0.as_u64();
        assert_eq!(10000, expected);
    }
}
