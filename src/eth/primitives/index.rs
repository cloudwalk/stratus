//! Index Module
//!
//! Responsible for representing transaction or log indexes within a block.
//! Indexes are essential for referencing specific transactions or logs in a
//! block, enabling efficient retrieval and referencing of these items. This
//! module includes functionality to handle conversions and representations of
//! indexes, aligning with Ethereum's blockchain data structure needs.

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
pub struct Index(u16);

impl Index {
    pub const ZERO: Index = Index(0u16);
    pub const ONE: Index = Index(1u16);

    pub fn new(inner: u16) -> Self {
        Index(inner)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = Index, other = u16);

impl From<i32> for Index {
    fn from(value: i32) -> Self {
        Index::new(value as u16)
    }
}

impl From<U64> for Index {
    fn from(value: U64) -> Self {
        Index::new(value.low_u64() as u16) // TODO: this will break things if the value is bigger than u16
    }
}

impl From<U256> for Index {
    fn from(value: U256) -> Self {
        Index::new(value.low_u64() as u16) // TODO: this will break things if the value is bigger than u16
    }
}

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
        // HACK: Actually SERIAL, sqlx was panicking
        sqlx::postgres::PgTypeInfo::with_name("INT4")
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for Index {
    fn encode_by_ref(&self, buf: &mut <sqlx::Postgres as HasArguments<'q>>::ArgumentBuffer) -> IsNull {
        <i32 as sqlx::Encode<sqlx::Postgres>>::encode(i32::from(self.0), buf)
    }
}

impl PgHasArrayType for Index {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        <i32 as PgHasArrayType>::array_type_info()
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

impl From<Index> for i32 {
    fn from(value: Index) -> Self {
        value.0.into()
    }
}
