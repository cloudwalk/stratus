pub use inmemory::InMemoryTemporaryStorage;
pub use inmemory::PendingBlockGuard;

mod inmemory;

use clap::Parser;
use display_json::DebugAsJson;

use super::BlockReference;
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
        let latest_sealed = perm_storage.read_chain_tip()?.unwrap_or_else(BlockReference::genesis);
        Ok(InMemoryTemporaryStorage::new(latest_sealed))
    }
}

pub fn compute_pending_block_number(perm_storage: &RocksPermanentStorage) -> anyhow::Result<BlockNumber> {
    Ok(perm_storage
        .read_chain_tip()?
        .unwrap_or_else(BlockReference::genesis)
        .number
        .next_block_number())
}
