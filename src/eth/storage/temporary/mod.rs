pub use inmemory::InMemoryTemporaryStorage;

mod inmemory;

use clap::Parser;
use display_json::DebugAsJson;

use super::RocksPermanentStorage;

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

/// Temporary storage configuration.
#[derive(Parser, DebugAsJson, Clone, serde::Serialize)]
pub struct TemporaryStorageConfig {
    // No configuration needed for InMemoryTemporaryStorage
}

impl TemporaryStorageConfig {
    /// Initializes temporary storage implementation.
    pub fn init(&self, perm_storage: &RocksPermanentStorage) -> anyhow::Result<InMemoryTemporaryStorage> {
        tracing::info!(config = ?self, "creating temporary storage");
        let pending_block_number = perm_storage.read_mined_block_number()? + 1;
        Ok(InMemoryTemporaryStorage::new(pending_block_number))
    }
}
