#[allow(clippy::module_inception)]
mod miner;
mod miner_config;
mod miner_error;

pub use miner::block_from_propagation;
pub use miner::Miner;
pub use miner_config::MinerConfig;
pub use miner_config::MinerMode;
pub use miner_error::MinerError;
