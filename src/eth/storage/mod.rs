//! Ethereum / EVM storage.

mod external_rpc_storage;
mod inmemory;
mod permanent_storage;
#[cfg(feature = "rocks")]
pub mod rocks;

mod storage_error;
mod stratus_storage;
mod temporary_storage;

pub use external_rpc_storage::ExternalRpcStorage;
pub use inmemory::InMemoryPermanentStorage;
pub use inmemory::InMemoryPermanentStorageState;
pub use inmemory::InMemoryTemporaryStorage;
pub use permanent_storage::PermanentStorage;
#[cfg(feature = "rocks")]
pub use rocks::rocks_permanent::RocksPermanentStorage;
pub use storage_error::StorageError;
pub use stratus_storage::StratusStorage;
pub use temporary_storage::TemporaryStorage;
