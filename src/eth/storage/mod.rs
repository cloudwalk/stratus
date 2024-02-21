//! Ethereum / EVM storage.

mod eth_storage;
mod inmemory;
mod permanent_storage;
mod postgres;
mod storage_error;
mod stratus_storage;
mod temporary_storage;

pub use inmemory::InMemoryStorage;
pub use permanent_storage::PermanentStorage;
pub use storage_error::EthStorageError;
pub use stratus_storage::StratusStorage;
pub use temporary_storage::TemporaryStorage;
