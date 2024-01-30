use std::str::FromStr;

use sqlx::database::HasValueRef;
use sqlx::error::BoxDynError;

/// Indicates how a transaction execution was finished.
#[derive(Debug, strum::Display, Clone, PartialEq, Eq, fake::Dummy, derive_new::new, serde::Serialize, serde::Deserialize, strum::EnumString)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionResult {
    /// Finished normally (RETURN opcode).
    #[strum(serialize = "success")]
    Success,

    /// Transaction execution finished with a reversion (REVERT opcode).
    #[strum(serialize = "reverted")]
    Reverted,

    /// Transaction execution did not finish.
    #[strum(serialize = "halted")]
    Halted { reason: String },
}

// -----------------------------------------------------------------------------
// Conversions: sqlx -> Self
// -----------------------------------------------------------------------------
impl<'r> sqlx::Decode<'r, sqlx::Postgres> for ExecutionResult {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <&str as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        Ok(ExecutionResult::from_str(value)?)
    }
}

impl sqlx::Type<sqlx::Postgres> for ExecutionResult {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("TEXT")
    }
}
