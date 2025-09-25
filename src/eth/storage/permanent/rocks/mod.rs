//! RocksDB layers (top-to-bottom): permanent -> state -> rest.

use std::fmt::Debug;

use anyhow::Context;
pub use rocks_cf_cache_config::RocksCfCacheConfig;
pub use rocks_permanent::RocksPermanentStorage;
pub use rocks_state::RocksStorageState;
use serde::Deserialize;
use serde::Serialize;

use crate::eth::storage::permanent::rocks::types::AddressRocksdb;
use crate::eth::storage::permanent::rocks::types::BlockNumberRocksdb;
use crate::eth::storage::permanent::rocks::types::SlotIndexRocksdb;

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

/// Column Family cache configuration.
pub mod rocks_cf_cache_config;

/// Functionalities related to the whole database.
pub mod rocks_db;

/// All types to be serialized and desserialized in the db.
pub mod types;

/// Utilities for testing.
#[cfg(test)]
pub mod test_utils;

// Tuple implementations for composite keys
impl SerializeDeserializeWithContext for (AddressRocksdb, BlockNumberRocksdb) {}
impl SerializeDeserializeWithContext for (AddressRocksdb, SlotIndexRocksdb) {}
impl SerializeDeserializeWithContext for (AddressRocksdb, SlotIndexRocksdb, BlockNumberRocksdb) {}

pub trait SerializeDeserializeWithContext {
    fn deserialize_with_context(bytes: &[u8]) -> anyhow::Result<Self>
    where
        Self: for<'de> Deserialize<'de> + bincode::Decode<()>,
    {
        use crate::rocks_bincode_config;
        let (result, _) = bincode::decode_from_slice(bytes, rocks_bincode_config())
            .with_context(|| format!("failed to deserialize '{}'", hex_fmt::HexFmt(bytes)))
            .with_context(|| format!("failed to deserialize to type '{}'", std::any::type_name::<Self>()))?;
        Ok(result)
    }

    fn serialize_with_context(input: &Self) -> anyhow::Result<Vec<u8>>
    where
        Self: Serialize + Debug + bincode::Encode,
    {
        use crate::rocks_bincode_config;
        bincode::encode_to_vec(input, rocks_bincode_config()).with_context(|| format!("failed to serialize '{input:?}'"))
    }
}
