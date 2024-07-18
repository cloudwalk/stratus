use chrono::DateTime;
use chrono::Utc;

/// DateTime that automatically sets the current time when created.
#[derive(Debug, Clone, derive_more::Deref, serde::Serialize)]
pub struct DateTimeNow(#[deref] DateTime<Utc>);

impl Default for DateTimeNow {
    fn default() -> Self {
        Self(Utc::now())
    }
}
