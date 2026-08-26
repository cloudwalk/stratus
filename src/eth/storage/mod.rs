//! Ethereum / EVM storage.

use cache::CacheConfig;
pub use cache::StorageCache;
pub use error::StorageError;
pub use permanent::PermanentStorageConfig;
pub use permanent::RocksPermanentStorage;
pub use stratus_storage::FoundAt;
pub use stratus_storage::MinedPointInTime;
pub use stratus_storage::StratusStorage;
pub use temporary::InMemoryTemporaryStorage;
pub use temporary::TemporaryStorageConfig;

mod cache;
mod error;
pub mod permanent;
mod resolve_pending;
mod stratus_storage;
mod temporary;

use std::sync::Arc;

use clap::Parser;
use display_json::DebugAsJson;
pub use temporary::compute_pending_block_number;

pub use crate::eth::types::ExecutionKind;
use crate::eth::types::StratusError;
pub use crate::eth::types::TxCount;

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

/// Configuration that can be used by any binary that interacts with Stratus storage.
#[derive(Parser, DebugAsJson, Clone, serde::Serialize)]
pub struct StorageConfig {
    #[clap(flatten)]
    pub temp_storage: TemporaryStorageConfig,

    #[clap(flatten)]
    pub perm_storage: PermanentStorageConfig,

    #[clap(flatten)]
    pub cache: CacheConfig,
}

impl StorageConfig {
    /// Initializes Stratus storage.
    pub fn init(&self) -> Result<Arc<StratusStorage>, StratusError> {
        let perm_storage = self.perm_storage.init()?;
        let temp_storage = self.temp_storage.init(&perm_storage)?;
        let cache = self.cache.init();

        let storage = StratusStorage::new(
            temp_storage,
            perm_storage,
            cache,
            #[cfg(feature = "dev")]
            self.perm_storage.clone(),
        )?;

        Ok(Arc::new(storage))
    }
}
