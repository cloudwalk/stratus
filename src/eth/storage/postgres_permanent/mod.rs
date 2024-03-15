#[allow(clippy::module_inception)]
mod postgres_permanent;
mod types;

pub use postgres_permanent::PostgresPermanentStorage;
pub use postgres_permanent::PostgresPermanentStorageConfig;
