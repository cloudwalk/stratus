//! RocksDB layers (top-to-bottom): permanent -> state -> rest.

use std::fmt::Debug;

use anyhow::Context;
pub use rocks_permanent::RocksPermanentStorage;
pub use rocks_state::RocksStorageState;
use serde::Deserialize;
use serde::Serialize;

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

pub trait SerializeDeserializeWithContext {
    fn deserialize_with_context(bytes: &[u8]) -> anyhow::Result<Self>
    where
        Self: for<'de> Deserialize<'de>,
    {
        bincode::deserialize::<Self>(bytes)
            .with_context(|| format!("failed to deserialize '{}'", hex_fmt::HexFmt(bytes)))
            .with_context(|| format!("failed to deserialize to type '{}'", std::any::type_name::<Self>()))
    }

    fn serialize_with_context(input: &Self) -> anyhow::Result<Vec<u8>>
    where
        Self: Serialize + Debug,
    {
        bincode::serialize(input).with_context(|| format!("failed to serialize '{input:?}'"))
    }
}
