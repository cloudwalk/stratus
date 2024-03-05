//! Unix Time Module
//!
//! Manages Unix time representation in the Ethereum ecosystem. Unix time, the
//! number of seconds since January 1, 1970, is used for timestamping blocks
//! and transactions in Ethereum. This module provides functionalities to
//! handle Unix time conversions and interactions, aligning with Ethereum's
//! time-based mechanisms and requirements.

use std::num::TryFromIntError;
use std::ops::Deref;
use std::str::FromStr;

use chrono::Utc;
use ethereum_types::U256;
use fake::Dummy;
use fake::Faker;
use revm::primitives::U256 as RevmU256;
use sqlx::database::HasValueRef;
use sqlx::error::BoxDynError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UnixTime(u64);

impl UnixTime {
    pub const ZERO: UnixTime = UnixTime(0u64);

    #[cfg(feature = "dev")]
    pub fn now() -> Self {
        offset::now()
    }

    #[cfg(feature = "dev")]
    pub fn set_offset(timestamp: UnixTime, latest_timestamp: UnixTime) -> anyhow::Result<()> {
        offset::set(timestamp, latest_timestamp)
    }

    #[cfg(not(feature = "dev"))]
    pub fn now() -> Self {
        Self(Utc::now().timestamp() as u64)
    }
}

impl FromStr for UnixTime {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        let without_prefix = s.trim_start_matches("0x");
        Ok(u64::from_str_radix(without_prefix, 16)?.into())
    }
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

impl From<U256> for UnixTime {
    fn from(value: U256) -> Self {
        value.low_u64().into()
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
        // HACK: should be "INTEGER" in theory
        // they are equal
        sqlx::postgres::PgTypeInfo::with_name("NUMERIC")
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<UnixTime> for RevmU256 {
    fn from(value: UnixTime) -> Self {
        Self::from_limbs([value.0, 0, 0, 0])
    }
}

impl TryFrom<UnixTime> for i64 {
    type Error = TryFromIntError;

    fn try_from(timestamp: UnixTime) -> Result<i64, TryFromIntError> {
        timestamp.0.try_into()
    }
}

impl TryFrom<UnixTime> for i32 {
    type Error = TryFromIntError;

    fn try_from(timestamp: UnixTime) -> Result<i32, TryFromIntError> {
        timestamp.0.try_into()
    }
}

#[cfg(feature = "dev")]
mod offset {
    use std::sync::atomic::AtomicI64;
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering::Acquire;
    use std::sync::atomic::Ordering::SeqCst;

    use super::*;

    pub static TIME_OFFSET: AtomicI64 = AtomicI64::new(0);
    pub static NEXT_TIMESTAMP: AtomicU64 = AtomicU64::new(0);

    pub fn set(timestamp: UnixTime, latest_timestamp: UnixTime) -> anyhow::Result<()> {
        use crate::log_and_err;
        let now = Utc::now().timestamp() as u64;

        if *timestamp != 0 && *timestamp < *latest_timestamp {
            return log_and_err!("timestamp can't be before the latest block");
        }

        let diff: i64 = if *timestamp == 0 { 0 } else { (*timestamp as i128 - now as i128) as i64 };
        NEXT_TIMESTAMP.store(*timestamp, SeqCst);
        TIME_OFFSET.store(diff, SeqCst);
        Ok(())
    }

    pub fn now() -> UnixTime {
        let offset_time = NEXT_TIMESTAMP.load(Acquire);
        let time_offset = TIME_OFFSET.load(Acquire);
        match offset_time {
            0 => UnixTime((Utc::now().timestamp() as i128 + time_offset as i128) as u64),
            _ => {
                let _ = NEXT_TIMESTAMP.fetch_update(SeqCst, SeqCst, |_| Some(0));
                UnixTime(offset_time)
            }
        }
    }
}
