//! Ethereum / EVM storage.

use cache::CacheConfig;
pub use cache::StorageCache;
pub use permanent::PermanentStorageConfig;
pub use permanent::RocksPermanentStorage;
pub use stratus_storage::StratusStorage;
pub use temporary::InMemoryTemporaryStorage;
pub use temporary::TemporaryStorageConfig;

mod cache;
pub mod permanent;
mod stratus_storage;
mod temporary;

use std::collections::HashMap;
use std::sync::Arc;

use clap::Parser;
use display_json::DebugAsJson;
pub use temporary::compute_pending_block_number;

use crate::eth::genesis::GenesisConfig;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
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
        let temp_storage = self.temp_storage.init(&perm_storage)?;
        let cache = self.cache.init();
        let genesis_config = if let Some(path) = &self.perm_storage.genesis_path {
            GenesisConfig::load_from_file(path)?
        } else {
            GenesisConfig::default()
        };

        let storage = StratusStorage::new(temp_storage, perm_storage, cache, genesis_config)?;

        Ok(Arc::new(storage))
    }
}

#[derive(Clone, Copy, PartialEq, Debug, serde::Serialize, serde::Deserialize, fake::Dummy, Eq)]
pub enum TxCount {
    Full,
    Partial(u64),
}

impl From<u64> for TxCount {
    fn from(value: u64) -> Self {
        TxCount::Partial(value)
    }
}

impl Default for TxCount {
    fn default() -> Self {
        TxCount::Partial(0)
    }
}

impl std::ops::AddAssign<u64> for TxCount {
    fn add_assign(&mut self, rhs: u64) {
        match self {
            TxCount::Full => {}                       // If it's Full, keep it Full
            TxCount::Partial(count) => *count += rhs, // If it's Partial, increment the counter
        }
    }
}

impl Ord for TxCount {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (TxCount::Full, TxCount::Full) => std::cmp::Ordering::Equal,
            (TxCount::Full, TxCount::Partial(_)) => std::cmp::Ordering::Greater,
            (TxCount::Partial(_), TxCount::Full) => std::cmp::Ordering::Less,
            (TxCount::Partial(a), TxCount::Partial(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for TxCount {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Default, fake::Dummy)]
pub enum ReadKind {
    Call((BlockNumber, TxCount)),
    #[default]
    Transaction,
    RPC,
}
