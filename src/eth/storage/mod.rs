//! Ethereum / EVM storage.

mod eth_storage;
mod inmemory;
mod metrified;
mod stratus_storage;
mod postgres;
mod storage_error;

pub use eth_storage::test_accounts;
pub use eth_storage::EthStorage;
pub use inmemory::InMemoryStorage;
pub use metrified::MetrifiedStorage;
pub use stratus_storage::StratusStorage;
pub use storage_error::EthStorageError;
