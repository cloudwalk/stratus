pub use inmemory::InMemoryTemporaryStorage;
pub use inmemory::ReadKind;
pub use inmemory::TxCount;

mod inmemory;

use clap::Parser;
use display_json::DebugAsJson;

use super::RocksPermanentStorage;
use crate::eth::primitives::BlockNumber;

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
        let perm_block_number = perm_storage.read_mined_block_number()?;
        let pending_block_number = if !perm_storage.has_genesis()? && perm_block_number == BlockNumber::ZERO {
            BlockNumber::ZERO
        } else {
            perm_block_number + 1
        };
        Ok(InMemoryTemporaryStorage::new(pending_block_number))
    }
}
