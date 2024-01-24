//! Unix Time Module
//!
//! Manages Unix time representation in the Ethereum ecosystem. Unix time, the
//! number of seconds since January 1, 1970, is used for timestamping blocks
//! and transactions in Ethereum. This module provides functionalities to
//! handle Unix time conversions and interactions, aligning with Ethereum's
//! time-based mechanisms and requirements.

use std::num::TryFromIntError;
use std::ops::Deref;

use fake::Dummy;
use fake::Faker;
use sqlx::database::HasValueRef;
use sqlx::error::BoxDynError;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UnixTime(u64);

impl UnixTime {
    pub const ZERO: UnixTime = UnixTime(0u64);
}

impl Deref for UnixTime {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Dummy<Faker> for UnixTime {
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(_: &Faker, rng: &mut R) -> Self {
        rng.next_u64().into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl From<u64> for UnixTime {
    fn from(value: u64) -> Self {
        UnixTime(value)
    }
}

// -----------------------------------------------------------------------------
// Conversions: sqlx -> Self
// -----------------------------------------------------------------------------

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for UnixTime {
    fn decode(value: <sqlx::Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        let value = <i64 as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        let value: u64 = value.try_into()?;
        Ok(value.into())
    }
}

impl sqlx::Type<sqlx::Postgres> for UnixTime {
    fn type_info() -> <sqlx::Postgres as sqlx::Database>::TypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("INTEGER")
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl TryFrom<UnixTime> for i64 {
    type Error = TryFromIntError;

    fn try_from(timestamp_in_secs: UnixTime) -> Result<i64, TryFromIntError> {
        timestamp_in_secs.0.try_into()
    }
}
