pub use inmemory::InMemoryTemporaryStorage;

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
        let pending_block_number = compute_pending_block_number(perm_storage)?;
        Ok(InMemoryTemporaryStorage::new(pending_block_number))
    }
}

pub fn compute_pending_block_number(perm_storage: &RocksPermanentStorage) -> anyhow::Result<BlockNumber> {
    let mined_block_number = perm_storage.read_mined_block_number()?;
    Ok(if !perm_storage.has_genesis()? && mined_block_number == BlockNumber::ZERO {
        BlockNumber::ZERO
    } else {
        mined_block_number + 1
    })
}
