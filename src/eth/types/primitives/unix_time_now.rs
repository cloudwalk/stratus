use display_json::DebugAsJson;

use crate::eth::storage::permanent::rocks::types::UnixTimeRocksdb;
use crate::eth::types::UnixTime;

/// [`UnixTime`] that automatically sets the current time when created.
#[derive(DebugAsJson, Clone, Copy, PartialEq, Eq, derive_more::Deref, serde::Serialize)]
#[cfg_attr(test, derive(serde::Deserialize, fake::Dummy))]
pub struct UnixTimeNow(#[deref] UnixTime);

impl Default for UnixTimeNow {
    fn default() -> Self {
        Self(UnixTime::now())
    }
}

impl From<UnixTime> for UnixTimeNow {
    fn from(value: UnixTime) -> Self {
        Self(value)
    }
}

impl From<UnixTimeRocksdb> for UnixTimeNow {
    fn from(value: UnixTimeRocksdb) -> Self {
        Self(value.into())
    }
}
