use std::str::FromStr;

use anyhow::anyhow;
use clap::Parser;
use display_json::DebugAsJson;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotSample;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::InMemoryPermanentStorage;
use crate::eth::storage::RocksPermanentStorage;

/// Permanent (committed) storage operations.
pub trait PermanentStorage: Send + Sync + 'static {
    // -------------------------------------------------------------------------
    // Block number
    // -------------------------------------------------------------------------

    /// Sets the last mined block number.
    fn set_mined_block_number(&self, number: BlockNumber) -> anyhow::Result<()>;

    // Retrieves the last mined block number.
    fn read_mined_block_number(&self) -> anyhow::Result<BlockNumber>;

    // -------------------------------------------------------------------------
    // Block
    // -------------------------------------------------------------------------

    /// Persists atomically all changes from a block.
    fn save_block(&self, block: Block) -> anyhow::Result<()>;

    /// Retrieves a block from the storage.
    fn read_block(&self, block_filter: &BlockFilter) -> anyhow::Result<Option<Block>>;

    /// Retrieves a transaction from the storage.
    fn read_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>>;

    /// Retrieves logs from the storage.
    fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>>;

    // -------------------------------------------------------------------------
    // Account and slots
    // -------------------------------------------------------------------------

    /// Persists initial accounts (test accounts or genesis accounts).
    fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()>;

    /// Retrieves an account from the storage. Returns Option when not found.
    fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>>;

    /// Retrieves an slot from the storage. Returns Option when not found.
    fn read_slot(&self, address: &Address, index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>>;

    /// Retrieves a random sample of slots, from the provided start and end blocks.
    fn read_slots_sample(&self, start: BlockNumber, end: BlockNumber, max_samples: u64, seed: u64) -> anyhow::Result<Vec<SlotSample>>;

    /// Retrieves all current slots associated to an address.
    fn read_all_slots(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Vec<Slot>>;

    // -------------------------------------------------------------------------
    // Global state
    // -------------------------------------------------------------------------

    /// Resets all state to a specific block number.
    fn reset_at(&self, number: BlockNumber) -> anyhow::Result<()>;
}

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

/// Permanent storage configuration.
#[derive(DebugAsJson, Clone, Parser, serde::Serialize)]
pub struct PermanentStorageConfig {
    /// Permamenent storage implementation.
    #[arg(long = "perm-storage", env = "PERM_STORAGE")]
    pub perm_storage_kind: PermanentStorageKind,

    /// RocksDB storage path prefix to execute multiple local Stratus instances.
    #[arg(long = "rocks-path-prefix", env = "ROCKS_PATH_PREFIX")]
    pub rocks_path_prefix: Option<String>,
}

#[derive(DebugAsJson, Clone, serde::Serialize)]
pub enum PermanentStorageKind {
    InMemory,
    Rocks,
}

impl PermanentStorageConfig {
    /// Initializes permanent storage implementation.
    pub fn init(&self) -> anyhow::Result<Box<dyn PermanentStorage>> {
        tracing::info!(config = ?self, "creating permanent storage");

        let perm: Box<dyn PermanentStorage> = match self.perm_storage_kind {
            PermanentStorageKind::InMemory => Box::<InMemoryPermanentStorage>::default(),
            PermanentStorageKind::Rocks => {
                let prefix = self.rocks_path_prefix.clone();
                Box::new(RocksPermanentStorage::new(prefix)?)
            }
        };
        Ok(perm)
    }
}

impl FromStr for PermanentStorageKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        match s {
            "inmemory" => Ok(Self::InMemory),
            "rocks" => Ok(Self::Rocks),
            s => Err(anyhow!("unknown permanent storage: {}", s)),
        }
    }
}
