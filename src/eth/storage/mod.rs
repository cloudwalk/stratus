//! Ethereum / EVM storage.

mod block_number_storage;
mod eth_storage;
pub mod inmemory;
pub use block_number_storage::BlockNumberStorage;
pub use eth_storage::EthStorage;
