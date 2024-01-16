//! Ethereum / EVM storage.

mod eth_storage;
mod metrified;
mod postgres;
mod impls;

pub use eth_storage::test_accounts;
pub use eth_storage::EthStorage;
pub use crate::eth::storage::impls::inmemory::InMemoryStorage;
pub use crate::eth::storage::impls::redis::RedisStorage;
pub use metrified::MetrifiedStorage;
