pub use inmemory::InMemoryTemporaryStorage;
pub use inmemory::PendingBlockGuard;

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
        Ok(InMemoryTemporaryStorage::new(perm_storage.read_chain_tip()?))
    }
}

pub fn compute_pending_block_number(perm_storage: &RocksPermanentStorage) -> anyhow::Result<BlockNumber> {
    Ok(perm_storage
        .read_chain_tip()?
        .map_or(BlockNumber::ZERO, |saved_tip| saved_tip.number.next_block_number()))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::eth::storage::permanent::RocksCfCacheConfig;

    #[test]
    fn empty_permanent_storage_initializes_pending_genesis() {
        let rocks_dir = tempfile::tempdir().expect("create rocks directory");
        let rocks_prefix = rocks_dir.path().join("empty-chain").to_string_lossy().into_owned();
        let permanent = RocksPermanentStorage::new(Some(rocks_prefix), Duration::from_secs(240), RocksCfCacheConfig::default(), true, None, 1024)
            .expect("create permanent storage");

        let temporary = TemporaryStorageConfig {}.init(&permanent).expect("create temporary storage");

        assert_eq!(temporary.read_pending_block_header().0.number, BlockNumber::ZERO);
        assert_eq!(
            compute_pending_block_number(&permanent).expect("compute pending block number"),
            BlockNumber::ZERO
        );
    }
}
