//! Index Module
//!
//! Responsible for representing transaction or log indexes within a block.
//! Indexes are essential for referencing specific transactions or logs in a
//! block, enabling efficient retrieval and referencing of these items. This
//! module includes functionality to handle conversions and representations of
//! indexes, aligning with Ethereum's blockchain data structure needs.

use sqlx::database::HasValueRef;
use sqlx::error::BoxDynError;

use crate::gen_newtype_from;

#[allow(dead_code)]
/// Representation of a transaction index or log index.
pub struct Index(u16);

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Index, other = u16);

// -----------------------------------------------------------------------------
// Conversions: sqlx -> Self
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for Index {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <i32 as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.into())
    }
}

impl sqlx::Type<sqlx::Postgres> for Index {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("SERIAL")
    }
}

impl From<i32> for Index {
    fn from(value: i32) -> Self {
        value.into()
    }
}
