//! RocksDB layers (top-to-bottom): permanent -> state -> rest.

pub use rocks_permanent::RocksPermanentStorage;
pub use rocks_state::RocksStorageState;

/// Exposed API.
mod rocks_permanent;

/// State handler for DB and column families.
pub mod rocks_state;

/// CFs versionated by value variant.
pub mod cf_versions;

/// Data manipulation for column families.
mod rocks_cf;

/// Settings and tweaks for the database and column families.
pub mod rocks_config;

/// Functionalities related to the whole database.
pub mod rocks_db;

/// All types to be serialized and desserialized in the db.
pub mod types;
