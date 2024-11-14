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

    #[cfg(feature = "dev")]
    pub fn evm_set_next_block_timestamp_was_called() -> bool {
        offset::evm_set_next_block_timestamp_was_called()
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
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::AtomicI64;
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering::Acquire;
    use std::sync::atomic::Ordering::SeqCst;

    use super::UnixTime;
    use super::Utc;

    /// Stores the time difference (in seconds) to apply to subsequent blocks
    /// This maintains relative time offsets after an explicit timestamp is used
    pub static NEW_TIMESTAMP_DIFF: AtomicI64 = AtomicI64::new(0);

    /// Stores the exact timestamp to use for the next block
    /// Only used once, then reset to 0
    pub static NEW_TIMESTAMP: AtomicU64 = AtomicU64::new(0);

    /// Indicates whether evm_setNextBlockTimestamp was called and hasn't been consumed yet
    pub static EVM_SET_NEXT_BLOCK_TIMESTAMP_WAS_CALLED: AtomicBool = AtomicBool::new(false);

    // Add this at the top with other static variables
    pub static LAST_BLOCK_TIMESTAMP: AtomicI64 = AtomicI64::new(0);

    /// Sets the timestamp for the next block and calculates the offset for subsequent blocks
    ///
    /// # Scenarios:
    /// 1. Setting a future timestamp:
    ///    - current_block = 100, new_timestamp = 110
    ///    - diff = (110 - current_time) = +10
    ///    - Next block will be exactly 110
    ///    - Subsequent blocks will maintain the +10 offset from current time
    ///
    /// 2. Setting timestamp to 0 (reset):
    ///    - Stores the current block timestamp for reference
    ///    - Resets the time offset to 0
    ///    - Next block will be current_block + 1
    ///    - Subsequent blocks use current time
    ///
    /// 3. Setting a past timestamp (error):
    ///    - current_block = 100, new_timestamp = 90
    ///    - Returns error: "timestamp can't be before the latest block"
    pub fn set(current_block_timestamp: UnixTime, new_block_timestamp: UnixTime) -> anyhow::Result<()> {
        use crate::log_and_err;
    
        if *new_block_timestamp == 0 {
            LAST_BLOCK_TIMESTAMP.store(*current_block_timestamp as i64, SeqCst);
        }
    
        if *new_block_timestamp != 0 && *new_block_timestamp < *current_block_timestamp {
            return log_and_err!("timestamp can't be before the latest block");
        }
    
        let current_time = Utc::now().timestamp() as i64;
        let diff: i64 = if *new_block_timestamp == 0 {
            0
        } else {
            (*new_block_timestamp as i64).saturating_sub(current_time)
        };
    
        NEW_TIMESTAMP.store(*new_block_timestamp, SeqCst);
        NEW_TIMESTAMP_DIFF.store(diff, SeqCst);
        EVM_SET_NEXT_BLOCK_TIMESTAMP_WAS_CALLED.store(true, SeqCst);
        Ok(())
    }

    /// Returns the timestamp for the current block based on various conditions.
    /// Ensures proper time progression by guaranteeing that each block's timestamp
    /// is at least 1 second greater than the previous block.
    ///
    /// # Behavior
    /// There are two main paths for timestamp generation:
    ///
    /// 1. When a specific timestamp was set (was_evm_timestamp_set = true):
    ///    - If new_timestamp is 0: Returns last_block_timestamp + 1
    ///    - If new_timestamp > 0: Returns exactly new_timestamp
    ///    After this call, resets the timestamp flag and stored value
    ///
    /// 2. For subsequent blocks (was_evm_timestamp_set = false):
    ///    - If new_timestamp is set: Uses it as reference point
    ///    - Otherwise: Uses max(current_time + offset, last_block_timestamp)
    ///    In both cases, adds 1 second to ensure progression
    ///
    /// The function always updates LAST_BLOCK_TIMESTAMP with the returned value
    /// to maintain the chain of increasing timestamps.
    pub fn now() -> UnixTime {
        let new_timestamp = NEW_TIMESTAMP.load(Acquire);
        let new_timestamp_diff = NEW_TIMESTAMP_DIFF.load(Acquire);
        let was_evm_timestamp_set = EVM_SET_NEXT_BLOCK_TIMESTAMP_WAS_CALLED.load(Acquire);
        let last_block_timestamp = LAST_BLOCK_TIMESTAMP.load(Acquire);
        let current_time = Utc::now().timestamp() as i64;
        
        let result = if !was_evm_timestamp_set {
            let last_timestamp = if new_timestamp != 0 {
                new_timestamp as i64
            } else {
                std::cmp::max(current_time + new_timestamp_diff, last_block_timestamp)
            };
    
            UnixTime((last_timestamp + 1) as u64)
        } else {
            EVM_SET_NEXT_BLOCK_TIMESTAMP_WAS_CALLED.store(false, SeqCst);
            NEW_TIMESTAMP.store(0, SeqCst);
            
            if new_timestamp == 0 {
                UnixTime((last_block_timestamp + 1) as u64)
            } else {
                UnixTime(new_timestamp)
            }
        };
    
        LAST_BLOCK_TIMESTAMP.store(*result as i64, SeqCst);
        result
    }

    /// Returns whether evm_setNextBlockTimestamp was called and hasn't been consumed yet
    pub fn evm_set_next_block_timestamp_was_called() -> bool {
        EVM_SET_NEXT_BLOCK_TIMESTAMP_WAS_CALLED.load(Acquire)
    }
}
