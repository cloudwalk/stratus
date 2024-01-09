use ethereum_types::Bloom;
use sqlx::database::HasValueRef;
use sqlx::error::BoxDynError;

struct LogsBloom(Bloom);

impl <'r> sqlx::Decode<'r, sqlx::Postgres> for LogsBloom {
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
