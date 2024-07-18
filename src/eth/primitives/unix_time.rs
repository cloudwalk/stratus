use std::num::TryFromIntError;
use std::ops::Deref;
use std::str::FromStr;

use chrono::Utc;
use ethereum_types::U256;
use fake::Dummy;
use fake::Faker;
use revm::primitives::U256 as RevmU256;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    pub fn set_offset(timestamp: UnixTime, latest_timestamp: UnixTime) -> anyhow::Result<()> {
        offset::set(timestamp, latest_timestamp)
    }

    pub fn to_i64(&self) -> i64 {
        self.0.try_into().expect("UNIX time is unrealistically high")
    }

    pub fn as_u64(&self) -> u64 {
        self.0
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
