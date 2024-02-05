//! Shared infrastructure.

pub mod blockchain_client;
pub mod metrics;
pub mod postgres;
pub mod tracing; // TODO: expose only struct, not module

pub use blockchain_client::BlockchainClient;
pub use metrics::init_metrics;
pub use tracing::init_tracing;
