use ethereum_types::U64;
use sqlx::database::HasValueRef;
use sqlx::error::BoxDynError;

use crate::gen_newtype_from;

// Type representing `v` variable
// from the ECDSA signature
pub struct EcdsaV(U64);

impl From<EcdsaV> for U64 {
    fn from(value: EcdsaV) -> Self {
        value.0
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
gen_newtype_from!(self = EcdsaV, other = i32);

// -----------------------------------------------------------------------------
// Conversions: sqlx -> Self
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for EcdsaV {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <i32 as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.into())
    }
}

impl sqlx::Type<sqlx::Postgres> for EcdsaV {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("BYTEA")
    }
}
