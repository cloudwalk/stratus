//! Ethereum / EVM storage.

mod inmemory;
mod permanent_storage;
mod postgres;
mod storage_error;
mod stratus_storage;
mod temporary_storage;

pub use inmemory::InMemoryStoragePermanent;
pub use inmemory::InMemoryStorageTemporary;
pub use permanent_storage::PermanentStorage;
pub use storage_error::StorageError;
pub use stratus_storage::StratusStorage;
pub use temporary_storage::TemporaryStorage;
