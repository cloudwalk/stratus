use std::str::FromStr;

use ethereum_types::U256;
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
pub struct Difficulty(U256);

impl Dummy<Faker> for Difficulty {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Difficulty, other = u8, u16, u32, u64, u128, U256, usize, i32, [u8; 32]);

impl TryFrom<BigDecimal> for Difficulty {
    type Error = anyhow::Error;

    fn try_from(value: BigDecimal) -> Result<Self, Self::Error> {
        let value_str = value.to_string();
        Ok(Difficulty(U256::from_dec_str(&value_str)?))
    }
}

// -----------------------------------------------------------------------------
// sqlx traits
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for Difficulty {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <BigDecimal as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.try_into()?)
    }
}

impl sqlx::Type<sqlx::Postgres> for Difficulty {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("NUMERIC")
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for Difficulty {
    fn encode_by_ref(&self, buf: &mut <sqlx::Postgres as HasArguments<'q>>::ArgumentBuffer) -> IsNull {
        match BigDecimal::try_from(*self) {
            Ok(res) => res.encode(buf),
            Err(err) => {
                tracing::error!(?err, "failed to encode gas");
                IsNull::Yes
            }
        }
    }
}

impl PgHasArrayType for Difficulty {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        <BigDecimal as PgHasArrayType>::array_type_info()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// ----------------------------------------------------------------------------
impl From<Difficulty> for U256 {
    fn from(value: Difficulty) -> Self {
        value.0
    }
}

impl From<Difficulty> for u64 {
    fn from(value: Difficulty) -> Self {
        value.0.low_u64()
    }
}

impl TryFrom<Difficulty> for BigDecimal {
    type Error = anyhow::Error;
    fn try_from(value: Difficulty) -> Result<Self, Self::Error> {
        // HACK: If we could import BigInt or BigUint we could convert the bytes directly.
        Ok(BigDecimal::from_str(&U256::from(value).to_string())?)
    }
}
