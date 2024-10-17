use display_json::DebugAsJson;

use crate::eth::primitives::UnixTime;

/// [`UnixTime`] that automatically sets the current time when created.
#[derive(DebugAsJson, Clone, PartialEq, Eq, derive_more::Deref, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct UnixTimeNow(#[deref] UnixTime);

impl Default for UnixTimeNow {
    fn default() -> Self {
        Self(UnixTime::now())
    }
}
