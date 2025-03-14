//! Ethereum / EVM storage.

use cache::CacheConfig;
pub use cache::StorageCache;
pub use permanent::InMemoryPermanentStorage;
pub use permanent::PermanentStorage;
pub use permanent::PermanentStorageConfig;
pub use permanent::PermanentStorageKind;
pub use stratus_storage::StratusStorage;
pub use temporary::InMemoryTemporaryStorage;
pub use temporary::TemporaryStorage;
pub use temporary::TemporaryStorageConfig;
pub use temporary::TemporaryStorageKind;

mod cache;
pub mod permanent;
mod stratus_storage;
mod temporary;

use std::collections::HashMap;
use std::sync::Arc;

use clap::Parser;
use display_json::DebugAsJson;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StratusError;

#[derive(Debug, Clone)]
pub struct AccountWithSlots {
    pub info: Account,
    pub slots: HashMap<SlotIndex, Slot, hash_hasher::HashBuildHasher>,
}

impl AccountWithSlots {
    /// Creates a new temporary account.
    fn new(address: Address) -> Self {
        Self {
            info: Account::new_empty(address),
            slots: HashMap::default(),
        }
    }
}

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
        let temp_storage = self.temp_storage.init(&*perm_storage)?;
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
