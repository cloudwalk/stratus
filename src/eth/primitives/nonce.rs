//! Nonce Module
//!
//! Manages nonces in Ethereum, which are crucial for preventing transaction
//! replay attacks. A nonce is a unique number assigned to each transaction sent
//! by an account, ensuring each transaction is processed once. This module
//! offers functionalities to create, manage, and convert nonces, maintaining
//! the integrity and uniqueness of transactions in the network.

use std::fmt::Display;

use anyhow::anyhow;
use ethereum_types::U256;
use ethereum_types::U64;
use fake::Dummy;
use fake::Faker;
use sqlx::database::HasArguments;
use sqlx::database::HasValueRef;
use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::postgres::PgHasArrayType;
use sqlx::types::BigDecimal;
use sqlx::Decode;

use crate::gen_newtype_from;
use crate::gen_newtype_try_from;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Nonce(U64);

impl Nonce {
    pub const ZERO: Nonce = Nonce(U64::zero());

    /// Checks if current value is zero.
    pub fn is_zero(&self) -> bool {
        self == &Self::ZERO
    }

    /// Returns the next nonce.
    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }
}

impl Display for Nonce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Dummy<Faker> for Nonce {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Nonce, other = u8, u16, u32, u64);
gen_newtype_try_from!(self = Nonce, other = i32);

impl TryFrom<BigDecimal> for Nonce {
    type Error = anyhow::Error;
    fn try_from(value: BigDecimal) -> Result<Self, Self::Error> {
        let value_str = value.to_string();

        Ok(Nonce(U64::from_str_radix(&value_str, 10)?))
    }
}

impl TryFrom<U256> for Nonce {
    type Error = anyhow::Error;

    fn try_from(value: U256) -> Result<Self, Self::Error> {
        Ok(Nonce(u64::try_from(value).map_err(|err| anyhow!(err))?.into()))
    }
}

// -----------------------------------------------------------------------------
// sqlx traits
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for Nonce {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <BigDecimal as Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.try_into()?)
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for Nonce {
    fn encode_by_ref(&self, buf: &mut <sqlx::Postgres as HasArguments<'q>>::ArgumentBuffer) -> IsNull {
        BigDecimal::from(self.0.as_u64()).encode(buf)
    }
}

impl PgHasArrayType for Nonce {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        <BigDecimal as PgHasArrayType>::array_type_info()
    }
}

impl sqlx::Type<sqlx::Postgres> for Nonce {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("NUMERIC")
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<Nonce> for u64 {
    fn from(value: Nonce) -> Self {
        value.0.as_u64()
    }
}

impl From<Nonce> for U256 {
    fn from(value: Nonce) -> Self {
        U256::from(value.0.as_u64())
    }
}

impl TryFrom<Nonce> for BigDecimal {
    type Error = anyhow::Error;
    fn try_from(value: Nonce) -> Result<Self, Self::Error> {
        // HACK: If we could import BigInt or BigUint we could convert the bytes directly.
        Ok(BigDecimal::from(value.0.as_u64()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn big_decimal_to_nonce_conversion() {
        // Test with a simple value
        let big_decimal = BigDecimal::new(1.into(), -4);
        let nonce: Nonce = big_decimal.clone().try_into().unwrap();
        let expected = nonce.0.as_u64();
        assert_eq!(10000, expected);
    }
}
