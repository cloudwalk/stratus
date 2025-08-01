mod metrics_config;
mod metrics_definitions;
mod metrics_types;

pub mod metrics_collector;
pub mod metrics_macros;

use std::time::Instant;

pub use metrics_collector::AsyncMetricsConfig;
pub use metrics_collector::AsyncMetricsStats;
pub use metrics_config::MetricsConfig;
pub use metrics_definitions::*;
pub use metrics_types::*;

pub fn now() -> Instant {
    Instant::now()
}
