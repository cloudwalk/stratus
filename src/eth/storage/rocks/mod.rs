//! RocksDB layers (top-to-bottom): permanent -> state -> rest.

/// Batch writer with capacity for flushing temporarily.
mod rocks_batch_writer;

/// Exposed API.
pub mod rocks_permanent;

/// State handler for DB and column families.
mod rocks_state;

/// CFs versionated by value variant.
mod cf_versions;
/// Data manipulation for column families.
pub mod rocks_cf;
/// Settings and tweaks for the database and column families.
mod rocks_config;
/// Functionalities related to the whole database.
mod rocks_db;
/// All types to be serialized and desserialized in the db.
mod types;
