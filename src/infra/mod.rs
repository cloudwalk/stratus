//! Shared infrastructure.

pub mod blockchain_client;
pub mod build_info;
pub mod docker;
#[cfg(feature = "metrics")]
pub mod metrics;
pub mod sentry;
pub mod tracing;

pub use blockchain_client::BlockchainClient;
#[cfg(feature = "metrics")]
pub use metrics::init_metrics;
pub use sentry::init_sentry;
pub use tracing::init_tracing;
