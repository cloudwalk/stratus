//! Ethereum / EVM storage.

pub mod eth_storage;
pub mod impls;
#[cfg(debug_assertions)]
pub use eth_storage::test_accounts;
pub use eth_storage::EthStorage;
pub use impls::inmemory::InMemoryStorage;
pub use impls::redis::RedisStorage;
