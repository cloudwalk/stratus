//! Ethereum / EVM storage.

pub mod eth_storage;
//pub mod inmemory;
pub mod impls;
pub use eth_storage::EthStorage;
pub use impls::inmemory::InMemoryStorage;
