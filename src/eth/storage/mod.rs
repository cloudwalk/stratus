//! Ethereum / EVM storage.

mod external_rpc_storage;
mod inmemory;
mod permanent_storage;
mod postgres_external_rpc;
pub mod rocks;

mod redis;
mod storage_point_in_time;
mod stratus_storage;
mod temporary_storage;

pub use external_rpc_storage::ExternalRpcStorage;
pub use external_rpc_storage::ExternalRpcStorageConfig;
pub use external_rpc_storage::ExternalRpcStorageKind;
pub use inmemory::InMemoryPermanentStorage;
pub use inmemory::InMemoryPermanentStorageState;
pub use inmemory::InMemoryTemporaryStorage;
pub use permanent_storage::PermanentStorage;
pub use permanent_storage::PermanentStorageConfig;
pub use permanent_storage::PermanentStorageKind;
pub use postgres_external_rpc::PostgresExternalRpcStorage;
pub use postgres_external_rpc::PostgresExternalRpcStorageConfig;
pub use rocks::rocks_permanent::RocksPermanentStorage;
pub use storage_point_in_time::StoragePointInTime;
pub use stratus_storage::StratusStorage;
pub use stratus_storage::StratusStorageConfig;
pub use temporary_storage::TemporaryStorage;
pub use temporary_storage::TemporaryStorageConfig;
pub use temporary_storage::TemporaryStorageKind;
