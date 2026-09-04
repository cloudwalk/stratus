/// Where a completed read obtained its value. Drives the post-read caching decision.
#[derive(Debug, Clone, Copy)]
pub enum FoundAt {
    /// Hit in a cache (pending or latest). Already cached; nothing to write.
    Cache,
    /// Found in temporary (pending or latest) storage.
    Temp,
    /// Read from permanent storage at the latest mined point.
    PermLatest,
    /// Read from permanent storage at a historical block.
    PermHistorical,
}

impl FoundAt {
    /// Stable identifier used as metrics label value.
    pub fn as_str(&self) -> &'static str {
        match self {
            FoundAt::Cache => "cache",
            FoundAt::Temp => "temp",
            FoundAt::PermLatest => "perm_latest",
            FoundAt::PermHistorical => "perm_historical",
        }
    }
}
