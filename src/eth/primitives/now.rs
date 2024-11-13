use chrono::DateTime;
use chrono::Utc;
use display_json::DebugAsJson;

/// DateTime that automatically sets the current time when created.
#[derive(DebugAsJson, Clone, PartialEq, Eq, derive_more::Deref, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct DateTimeNow(#[deref] DateTime<Utc>);

impl Default for DateTimeNow {
    fn default() -> Self {
        Self(Utc::now())
    }
}
