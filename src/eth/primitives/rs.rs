use ethereum_types::U256;
use sqlx::database::HasValueRef;
use sqlx::error::BoxDynError;

use crate::gen_newtype_from;

// Type representing `r` and `s` variables
// from the ECDSA signature
pub struct Rs(U256);

impl From<Rs> for U256 {
    fn from(value: Rs) -> Self {
        value.0
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Rs, other = i64);

// -----------------------------------------------------------------------------
// Conversions: sqlx -> Self
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for Rs {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <i64 as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.into())
    }
}

impl sqlx::Type<sqlx::Postgres> for Rs {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("BYTEA")
    }
}
