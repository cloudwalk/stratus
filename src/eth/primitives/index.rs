//! Index Module
//!
//! Responsible for representing transaction or log indexes within a block.
//! Indexes are essential for referencing specific transactions or logs in a
//! block, enabling efficient retrieval and referencing of these items. This
//! module includes functionality to handle conversions and representations of
//! indexes, aligning with Ethereum's blockchain data structure needs.

use ethereum_types::{U256, U64};
use sqlx::database::HasValueRef;
use ethereum_types::U256;
use ethereum_types::U64;
use sqlx::error::BoxDynError;

use crate::gen_newtype_from;

/// Represents a transaction index or log index.
#[derive(Debug, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize, derive_more::Add, Copy)]
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
