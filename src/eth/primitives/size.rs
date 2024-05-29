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

#[derive(Debug, derive_more::Display, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Size(U64);

impl Dummy<Faker> for Size {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Size, other = u8, u16, u32, u64);

impl TryFrom<BigDecimal> for Size {
    type Error = anyhow::Error;

    fn try_from(value: BigDecimal) -> Result<Self, Self::Error> {
        let value_str = value.to_string();
        Ok(Size(U64::from_str_radix(&value_str, 10)?))
    }
}

impl TryFrom<U256> for Size {
    type Error = anyhow::Error;

    fn try_from(value: U256) -> Result<Self, Self::Error> {
        Ok(Size(u64::try_from(value).map_err(|err| anyhow!(err))?.into()))
    }
}

// -----------------------------------------------------------------------------
// sqlx traits
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for Size {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <BigDecimal as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.try_into()?)
    }
}

impl sqlx::Type<sqlx::Postgres> for Size {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("NUMERIC")
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for Size {
    fn encode_by_ref(&self, buf: &mut <sqlx::Postgres as HasArguments<'q>>::ArgumentBuffer) -> IsNull {
        BigDecimal::from(*self).encode(buf)
    }
}

impl PgHasArrayType for Size {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        <BigDecimal as PgHasArrayType>::array_type_info()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// ----------------------------------------------------------------------------
impl From<Size> for U256 {
    fn from(value: Size) -> Self {
        value.0.as_u64().into()
    }
}

impl From<Size> for u64 {
    fn from(value: Size) -> Self {
        value.0.as_u64()
    }
}

impl From<Size> for BigDecimal {
    fn from(value: Size) -> Self {
        BigDecimal::from(value.0.as_u64())
    }
}
