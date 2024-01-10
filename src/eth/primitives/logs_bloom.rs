use std::ops::Deref;
use std::ops::DerefMut;

use ethereum_types::Bloom;
use sqlx::database::HasValueRef;
use sqlx::error::BoxDynError;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(transparent)]
pub struct LogsBloom(Bloom);

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for LogsBloom {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <LogsBloom as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        Ok(value)
    }
}

impl sqlx::Type<sqlx::Postgres> for LogsBloom {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("BYTEA")
    }
}

impl Deref for LogsBloom {
    type Target = Bloom;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for LogsBloom {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
