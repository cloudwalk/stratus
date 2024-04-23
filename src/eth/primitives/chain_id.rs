//! Chain ID Module
//!
//! The Chain ID module provides a unique identifier for different Ethereum
//! networks, such as Mainnet, Ropsten, or Rinkeby. This identifier is crucial
//! in transaction signing to prevent replay attacks across different networks.
//! The module enables the specification and verification of the network for
//! which a particular transaction is intended.

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

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChainId(U64);

impl ChainId {
    pub fn new(value: U64) -> Self {
        Self(value)
    }

    pub fn inner_value(&self) -> U64 {
        self.0
    }
}

impl Display for ChainId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Dummy<Faker> for ChainId {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = ChainId, other = u8, u16, u32, u64);
gen_newtype_try_from!(self = ChainId, other = i32);

impl TryFrom<U256> for ChainId {
    type Error = anyhow::Error;

    fn try_from(value: U256) -> Result<Self, Self::Error> {
        Ok(ChainId(u64::try_from(value).map_err(|err| anyhow!(err))?.into()))
    }
}

impl TryFrom<BigDecimal> for ChainId {
    type Error = anyhow::Error;
    fn try_from(value: BigDecimal) -> Result<Self, Self::Error> {
        let value_str = value.to_string();

        Ok(ChainId(U64::from_str_radix(&value_str, 10)?))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<ChainId> for u64 {
    fn from(value: ChainId) -> Self {
        value.0.as_u64()
    }
}

impl From<ChainId> for U256 {
    fn from(value: ChainId) -> Self {
        value.0.as_u64().into()
    }
}

impl TryFrom<ChainId> for BigDecimal {
    type Error = anyhow::Error;
    fn try_from(value: ChainId) -> Result<Self, Self::Error> {
        // HACK: If we could import BigInt or BigUint we could convert the bytes directly.
        Ok(BigDecimal::from(value.0.as_u64()))
    }
}

// -----------------------------------------------------------------------------
// sqlx traits
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for ChainId {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <BigDecimal as Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.try_into()?)
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for ChainId {
    fn encode_by_ref(&self, buf: &mut <sqlx::Postgres as HasArguments<'q>>::ArgumentBuffer) -> IsNull {
        BigDecimal::from(self.0.as_u64()).encode(buf)
    }
}

impl PgHasArrayType for ChainId {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        <BigDecimal as PgHasArrayType>::array_type_info()
    }
}

impl sqlx::Type<sqlx::Postgres> for ChainId {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("NUMERIC")
    }
}
