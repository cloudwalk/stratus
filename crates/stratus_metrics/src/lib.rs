//! Stratus metrics infrastructure.
//!
//! This crate provides the metric definitions, label types, timing helpers, and
//! the metrics exporter configuration used to record Stratus metrics. The
//! Stratus-specific glue (`EvmKind` labels, executor pool gauges) lives in
//! `stratus::infra::metrics`.

mod config;
mod definitions;
mod types;

use std::future::Future;
use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;

pub use config::MetricsConfig;
pub use definitions::*;
pub use stratus_metrics_macros::timed;
pub use types::*;

/// Provider for the `node_mode` label attached to every metric.
static NODE_MODE_PROVIDER: OnceLock<Box<dyn Fn() -> String + Send + Sync>> = OnceLock::new();

/// Sets the provider used to fill the `node_mode` label present in every metric.
///
/// The provider is queried each time a metric is recorded, so it must read the
/// current node mode instead of caching it.
pub fn set_node_mode_provider(provider: impl Fn() -> String + Send + Sync + 'static) {
    let _ = NODE_MODE_PROVIDER.set(Box::new(provider));
}

/// Current value for the `node_mode` label, or `None` when no provider is set.
pub(crate) fn node_mode() -> MetricLabelValue {
    NODE_MODE_PROVIDER
        .get()
        .map_or(MetricLabelValue::None, |provider| MetricLabelValue::Some(provider()))
}

/// Track metrics execution starting instant.
pub fn now() -> Instant {
    Instant::now()
}

/// Executes an operation, publishes its elapsed time and result, and returns
/// the result unchanged.
pub fn record<T>(operation: impl FnOnce() -> T, publish: impl FnOnce(Duration, &T)) -> T {
    let start = now();
    let result = operation();
    publish(start.elapsed(), &result);
    result
}

/// Async variant of [`record`].
pub async fn record_async<T, F>(operation: impl FnOnce() -> F, publish: impl FnOnce(Duration, &T)) -> T
where
    F: Future<Output = T>,
{
    let start = now();
    let result = operation().await;
    publish(start.elapsed(), &result);
    result
}
