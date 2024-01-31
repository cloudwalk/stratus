//! Gas Module
//!
//! In Ethereum, the Gas module is responsible for representing and managing
//! gas, the unit used to measure the computational effort required to execute
//! operations. It plays a critical role in Ethereum's transaction model,
//! ensuring network security and preventing abuse by assigning a cost to every
//! computation and storage operation. The module provides functionalities to
//! calculate, track, and limit gas usage in transactions and smart contract
//! execution.

use std::fmt::Display;
use std::str::FromStr;

use ethereum_types::U256;
use fake::Dummy;
use fake::Faker;
use sqlx::database::HasValueRef;
use sqlx::error::BoxDynError;
use sqlx::types::BigDecimal;

use crate::gen_newtype_from;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Gas(U256);

impl Gas {
    pub const ZERO: Gas = Gas(U256::zero());
}

impl Display for Gas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Dummy<Faker> for Gas {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Gas, other = u8, u16, u32, u64, u128, U256, usize, i32, [u8; 32]);

impl From<BigDecimal> for Gas {
    fn from(value: BigDecimal) -> Self {
        // NOTE: This clones, but there I found no other way to get the BigInt (or the bytes) in BigDecimal
        let (integer, _) = value.as_bigint_and_exponent();
        let (_, bytes) = integer.to_bytes_be();
        Gas(U256::from_big_endian(&bytes))
    }
}

// -----------------------------------------------------------------------------
// Conversions: sqlx -> Self
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for Gas {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <BigDecimal as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.into())
    }
}

impl sqlx::Type<sqlx::Postgres> for Gas {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("NUMERIC")
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// ----------------------------------------------------------------------------
impl From<Gas> for U256 {
    fn from(value: Gas) -> Self {
        value.0
    }
}

impl From<Gas> for usize {
    fn from(value: Gas) -> Self {
        value.0.as_usize()
    }
}

impl TryFrom<Gas> for BigDecimal {
    type Error = anyhow::Error;
    fn try_from(value: Gas) -> Result<Self, Self::Error> {
        // HACK: If we could import BigInt or BigUint we could convert the bytes directly.
        Ok(BigDecimal::from_str(&U256::from(value).to_string())?)
    }
}
