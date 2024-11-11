use std::num::TryFromIntError;
use std::ops::Deref;
use std::str::FromStr;

use chrono::DateTime;
use chrono::Utc;
use display_json::DebugAsJson;
use ethereum_types::U256;
use fake::Dummy;
use fake::Faker;

use crate::alias::RevmU256;
use crate::ext::InfallibleExt;

#[derive(DebugAsJson, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UnixTime(u64);

impl UnixTime {
    pub const ZERO: UnixTime = UnixTime(0u64);

    #[cfg(not(feature = "dev"))]
    pub fn now() -> Self {
        Self(Utc::now().timestamp() as u64)
    }

    #[cfg(feature = "dev")]
    pub fn now() -> Self {
        offset::now()
    }

    #[cfg(feature = "dev")]
    pub fn set_offset(current_block_timestamp: UnixTime, new_block_timestamp: UnixTime) -> anyhow::Result<()> {
        offset::set(current_block_timestamp, new_block_timestamp)
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
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<UnixTime> for RevmU256 {
    fn from(value: UnixTime) -> Self {
        Self::from_limbs([value.0, 0, 0, 0])
    }
}

impl From<UnixTime> for DateTime<Utc> {
    fn from(value: UnixTime) -> Self {
        DateTime::from_timestamp(value.0 as i64, 0).expect_infallible()
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

    use super::UnixTime;
    use super::Utc;

    pub static NEW_TIMESTAMP_DIFF: AtomicI64 = AtomicI64::new(0);
    pub static NEW_TIMESTAMP: AtomicU64 = AtomicU64::new(0);

    pub fn set(current_block_timestamp: UnixTime, new_block_timestamp: UnixTime) -> anyhow::Result<()> {
        use crate::log_and_err;
        let now = Utc::now().timestamp() as u64;

        if *new_block_timestamp != 0 && *new_block_timestamp < *current_block_timestamp {
            return log_and_err!("timestamp can't be before the latest block");
        }

        let diff: i64 = if *new_block_timestamp == 0 { 0 } else { (*new_block_timestamp as i128 - now as i128) as i64 };
        NEW_TIMESTAMP.store(*new_block_timestamp, SeqCst);
        NEW_TIMESTAMP_DIFF.store(diff, SeqCst);
        Ok(())
    }

    pub fn now() -> UnixTime {
        let new_timestamp = NEW_TIMESTAMP.load(Acquire);
        let new_timestamp_diff = NEW_TIMESTAMP_DIFF.load(Acquire);
        match new_timestamp {
            0 => UnixTime((Utc::now().timestamp() as i128 + new_timestamp_diff as i128) as u64),
            _ => {
                let _ = NEW_TIMESTAMP.fetch_update(SeqCst, SeqCst, |_| Some(0));
                UnixTime(new_timestamp)
            }
        }
    }
}
