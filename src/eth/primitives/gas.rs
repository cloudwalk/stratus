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

use crate::gen_newtype_from;
use crate::gen_newtype_try_from;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Gas(U64);

impl Gas {
    pub const ZERO: Gas = Gas(U64::zero());
    pub const MAX: Gas = Gas(U64::MAX);

    pub fn as_u64(&self) -> u64 {
        self.0.as_u64()
    }
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
gen_newtype_from!(self = Gas, other = u8, u16, u32, u64);
gen_newtype_try_from!(self = Gas, other = i32);

impl TryFrom<BigDecimal> for Gas {
    type Error = anyhow::Error;

    fn try_from(value: BigDecimal) -> Result<Self, Self::Error> {
        let value_str = value.to_string();
        Ok(Gas(U64::from_str_radix(&value_str, 10)?))
    }
}

impl TryFrom<U256> for Gas {
    type Error = anyhow::Error;

    fn try_from(value: U256) -> Result<Self, Self::Error> {
        Ok(Gas(u64::try_from(value).map_err(|err| anyhow!(err))?.into()))
    }
}

// -----------------------------------------------------------------------------
// sqlx traits
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for Gas {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <BigDecimal as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.try_into()?)
    }
}

impl sqlx::Type<sqlx::Postgres> for Gas {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("NUMERIC")
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for Gas {
    fn encode_by_ref(&self, buf: &mut <sqlx::Postgres as HasArguments<'q>>::ArgumentBuffer) -> IsNull {
        BigDecimal::from(*self).encode(buf)
    }
}

impl PgHasArrayType for Gas {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        <BigDecimal as PgHasArrayType>::array_type_info()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// ----------------------------------------------------------------------------
impl From<Gas> for U256 {
    fn from(value: Gas) -> Self {
        value.0.as_u64().into()
    }
}

impl From<Gas> for u64 {
    fn from(value: Gas) -> Self {
        value.0.as_u64()
    }
}

impl From<Gas> for BigDecimal {
    fn from(value: Gas) -> BigDecimal {
        BigDecimal::from(value.0.as_u64())
    }
}
