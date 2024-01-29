//! Nonce Module
//!
//! Manages nonces in Ethereum, which are crucial for preventing transaction
//! replay attacks. A nonce is a unique number assigned to each transaction sent
//! by an account, ensuring each transaction is processed once. This module
//! offers functionalities to create, manage, and convert nonces, maintaining
//! the integrity and uniqueness of transactions in the network.

use std::fmt::Display;

use ethereum_types::U256;
use fake::Dummy;
use fake::Faker;
use sqlx::database::HasValueRef;
use sqlx::error::BoxDynError;
use sqlx::types::BigDecimal;
use sqlx::Decode;

use crate::gen_newtype_from;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Nonce(U256);

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
gen_newtype_from!(self = Nonce, other = u8, u16, u32, u64, u128, U256, usize, i32);

impl From<BigDecimal> for Nonce {
    fn from(value: BigDecimal) -> Self {
        let (integer, _) = value.as_bigint_and_exponent();
        let (_, bytes) = integer.to_bytes_be();
        Nonce(U256::from_big_endian(&bytes))
    }
}

// -----------------------------------------------------------------------------
// Conversions: sqlx -> Self
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for Nonce {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <BigDecimal as Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.into())
    }
}

impl<'r> sqlx::Encode<'r, sqlx::Postgres> for Nonce {
    fn encode(self, buf: &mut <sqlx::Postgres as sqlx::database::HasArguments<'r>>::ArgumentBuffer) -> sqlx::encode::IsNull
    where
        Self: Sized,
    {
        BigDecimal::parse_bytes(&<[u8; 32]>::from(self.0), 10).encode(buf)
    }

    fn encode_by_ref(&self, buf: &mut <sqlx::Postgres as sqlx::database::HasArguments<'r>>::ArgumentBuffer) -> sqlx::encode::IsNull {
        BigDecimal::parse_bytes(&<[u8; 32]>::from(self.0), 10).encode(buf)
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
        value.0
    }
}

impl From<Nonce> for BigDecimal {
    fn from(value: Nonce) -> Self {
        BigDecimal::parse_bytes(&<[u8; 32]>::from(U256::from(value)), 10).unwrap_or(BigDecimal::from(0))
    }
}
