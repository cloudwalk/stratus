use std::collections::HashSet;

use sqlx::database::HasArguments;
use sqlx::database::HasValueRef;
use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::postgres::PgHasArrayType;
use sqlx::types::Json;
use sqlx::Decode;

use crate::eth::primitives::SlotIndex;

#[derive(Debug, Clone, Default, PartialEq, Eq, fake::Dummy, serde::Deserialize, serde::Serialize, derive_more::Deref, derive_more::DerefMut)]
pub struct SlotIndexes(#[deref] pub HashSet<SlotIndex>);

impl SlotIndexes {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self(HashSet::with_capacity(capacity))
    }
}

// -----------------------------------------------------------------------------
// sqlx traits
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for SlotIndexes {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <Json<SlotIndexes> as Decode<sqlx::Postgres>>::decode(value)?.0;
        Ok(value)
    }
}

impl sqlx::Type<sqlx::Postgres> for SlotIndexes {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("JSONB")
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for SlotIndexes {
    fn encode_by_ref(&self, buf: &mut <sqlx::Postgres as HasArguments<'q>>::ArgumentBuffer) -> IsNull {
        <Json<SlotIndexes> as sqlx::Encode<sqlx::Postgres>>::encode(self.clone().into(), buf)
    }

    fn encode(self, buf: &mut <sqlx::Postgres as HasArguments<'q>>::ArgumentBuffer) -> IsNull
    where
        Self: Sized,
    {
        <Json<SlotIndexes> as sqlx::Encode<sqlx::Postgres>>::encode(self.into(), buf)
    }
}

impl PgHasArrayType for SlotIndexes {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        <Json<SlotIndexes> as PgHasArrayType>::array_type_info()
    }
}
