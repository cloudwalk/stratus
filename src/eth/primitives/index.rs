//! Index Module
//!
//! Responsible for representing transaction or log indexes within a block.
//! Indexes are essential for referencing specific transactions or logs in a
//! block, enabling efficient retrieval and referencing of these items. This
//! module includes functionality to handle conversions and representations of
//! indexes, aligning with Ethereum's blockchain data structure needs.

use std::num::TryFromIntError;

use ethereum_types::U256;
use ethereum_types::U64;
use sqlx::database::HasArguments;
use sqlx::database::HasValueRef;
use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::postgres::PgHasArrayType;

use crate::gen_newtype_from;

/// Represents a transaction index or log index.
///
/// TODO: representing it as u16 is probably wrong because external libs uses u64.
#[derive(Debug, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize, derive_more::Add, Copy, Hash)]
pub struct Index(u64);

impl Index {
    pub const ZERO: Index = Index(0u64);
    pub const ONE: Index = Index(1u64);

    pub fn new(inner: u64) -> Self {
        Index(inner)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Index, other = u64);

impl From<i64> for Index {
    fn from(value: i64) -> Self {
        Index::new(value as u64)
    }
}

impl From<U64> for Index {
    fn from(value: U64) -> Self {
        Index::new(value.low_u64() as u64) // TODO: this will break things if the value is bigger than u16
    }
}

impl From<U256> for Index {
    fn from(value: U256) -> Self {
        Index::new(value.low_u64() as u64) // TODO: this will break things if the value is bigger than u16
    }
}

// -----------------------------------------------------------------------------
// sqlx traits
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for Index {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <i64 as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value.into())
    }
}

impl sqlx::Type<sqlx::Postgres> for Index {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        // HACK: Actually SERIAL, sqlx was panicking
        sqlx::postgres::PgTypeInfo::with_name("INT8")
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for Index {
    fn encode_by_ref(&self, buf: &mut <sqlx::Postgres as HasArguments<'q>>::ArgumentBuffer) -> IsNull {
        let integer = match i64::try_from(self.0) {
            Ok(val) => val,
            Err(err) => {
                tracing::error!(reason = ?err, "failed to convert Index to i64");
                return IsNull::Yes
            }
        };

        <i64 as sqlx::Encode<sqlx::Postgres>>::encode(integer, buf)
    }
}

impl PgHasArrayType for Index {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        <i64 as PgHasArrayType>::array_type_info()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<Index> for U64 {
    fn from(value: Index) -> U64 {
        value.0.into()
    }
}

impl From<Index> for U256 {
    fn from(value: Index) -> U256 {
        value.0.into()
    }
}

impl TryFrom<Index> for i32 {
    type Error = TryFromIntError;

    fn try_from(value: Index) -> Result<Self, Self::Error> {
        value.0.try_into()
    }
}
