mod metrics_config;
mod metrics_definitions;
mod metrics_macros;
mod metrics_types;

use std::time::Instant;

pub use metrics_config::MetricsConfig;
pub use metrics_definitions::*;
pub use metrics_types::*;

/// Track metrics execution starting instant.
pub fn now() -> Instant {
    Instant::now()
}
