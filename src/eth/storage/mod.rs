//! Ethereum / EVM storage.

pub mod permanent;
mod stratus_storage;
mod temporary;

pub use permanent::InMemoryPermanentStorage;
pub use permanent::PermanentStorage;
pub use permanent::PermanentStorageConfig;
pub use permanent::PermanentStorageKind;
pub use stratus_storage::StratusStorage;
pub use stratus_storage::StratusStorageConfig;
pub use temporary::InMemoryTemporaryStorage;
pub use temporary::TemporaryStorage;
pub use temporary::TemporaryStorageConfig;
pub use temporary::TemporaryStorageKind;
