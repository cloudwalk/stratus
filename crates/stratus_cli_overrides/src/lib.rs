//! Shared trait for merging explicitly provided CLI arguments over config file values.
//!
//! Config structs implement [`CliOverrides`] via the `CliOverrides` derive from `stratus_macros`.
//! The trait lives in its own crate — instead of in `stratus` itself — so config structs owned by
//! other workspace crates, such as `stratus_metrics::MetricsConfig`, can participate in the
//! CLI-over-file merge performed by `stratus`'s configuration loader.

use std::collections::HashSet;

/// Merges values from explicitly provided CLI arguments over values loaded from the config file.
///
/// The derive from `stratus_macros` generates the implementation from the struct fields: plain
/// fields are copied when their argument was explicitly provided in the command line, flattened
/// sections recurse into the child struct, and serde-skipped fields are always taken from the CLI.
pub trait CliOverrides {
    /// Applies `cli` values over `self`, restricted to the arguments in `explicit`.
    fn apply_cli_overrides(&mut self, cli: &Self, explicit: &HashSet<String>);
}
