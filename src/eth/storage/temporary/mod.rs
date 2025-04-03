pub use inmemory::InMemoryTemporaryStorage;
use strum::VariantNames;

mod inmemory;

use std::str::FromStr;

use anyhow::anyhow;
use clap::Parser;
use display_json::DebugAsJson;

use super::PermanentStorage;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
#[cfg(feature = "dev")]
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Hash;
#[cfg(feature = "dev")]
use crate::eth::primitives::Nonce;
use crate::eth::primitives::PendingBlock;
use crate::eth::primitives::PendingBlockHeader;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StorageError;
use crate::eth::primitives::TransactionExecution;
#[cfg(feature = "dev")]
use crate::eth::primitives::Wei;

/// Temporary storage (in-between blocks) operations.
pub trait TemporaryStorage: Send + Sync + 'static {
    // -------------------------------------------------------------------------
    // Block header
    // -------------------------------------------------------------------------

    // Retrieves the block number being mined.
    fn read_pending_block_header(&self) -> PendingBlockHeader;

    // -------------------------------------------------------------------------
    // Block and executions
    // -------------------------------------------------------------------------

    /// Finishes the mining of the pending block and starts a new block.
    fn finish_pending_block(&self) -> anyhow::Result<PendingBlock, StorageError>;

    /// Saves a transaction execution to the pending mined block.
    fn save_pending_execution(&self, tx: TransactionExecution, check_conflicts: bool) -> Result<(), StorageError>;

    /// Retrieves all transaction executions from the pending block.
    fn read_pending_executions(&self) -> Vec<TransactionExecution>;

    /// Retrieves a single transaction execution from the pending block.
    fn read_pending_execution(&self, hash: Hash) -> anyhow::Result<Option<TransactionExecution>, StorageError>;

    // -------------------------------------------------------------------------
    // Accounts and slots
    // -------------------------------------------------------------------------

    /// Retrieves an account from the storage. Returns Option when not found.
    fn read_account(&self, address: Address) -> anyhow::Result<Option<Account>, StorageError>;

    /// Retrieves an slot from the storage. Returns Option when not found.
    fn read_slot(&self, address: Address, index: SlotIndex) -> anyhow::Result<Option<Slot>, StorageError>;

    // -------------------------------------------------------------------------
    // Direct state manipulation (for testing)
    // -------------------------------------------------------------------------

    #[cfg(feature = "dev")]
    /// Saves a single slot directly to temporary storage.
    fn save_slot(&self, address: Address, slot: Slot) -> anyhow::Result<(), StorageError>;

    #[cfg(feature = "dev")]
    /// Updates an account's nonce in temporary storage.
    fn save_account_nonce(&self, address: Address, nonce: Nonce) -> anyhow::Result<(), StorageError>;

    #[cfg(feature = "dev")]
    /// Updates an account's balance in temporary storage.
    fn save_account_balance(&self, address: Address, balance: Wei) -> anyhow::Result<(), StorageError>;

    #[cfg(feature = "dev")]
    /// Updates an account's code in temporary storage.
    fn save_account_code(&self, address: Address, code: Bytes) -> anyhow::Result<(), StorageError>;

    // -------------------------------------------------------------------------
    // Global state
    // -------------------------------------------------------------------------

    /// Resets to default empty state.
    fn reset(&self) -> anyhow::Result<(), StorageError>;
}

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

/// Temporary storage configuration.
#[derive(Parser, DebugAsJson, Clone, serde::Serialize)]
pub struct TemporaryStorageConfig {
    /// Temporary storage implementation.
    #[arg(long = "temp-storage", env = "TEMP_STORAGE", default_value = "inmemory")]
    pub temp_storage_kind: TemporaryStorageKind,
}

#[derive(DebugAsJson, strum::Display, strum::VariantNames, Clone, Copy, Parser, serde::Serialize)]
pub enum TemporaryStorageKind {
    #[serde(rename = "inmemory")]
    #[strum(to_string = "inmemory")]
    InMemory,
}

impl TemporaryStorageConfig {
    /// Initializes temporary storage implementation.
    pub fn init(&self, perm_storage: &dyn PermanentStorage) -> anyhow::Result<Box<dyn TemporaryStorage>> {
        tracing::info!(config = ?self, "creating temporary storage");
        let pending_block_number = perm_storage.read_mined_block_number()? + 1;
        match self.temp_storage_kind {
            TemporaryStorageKind::InMemory => Ok(Box::new(InMemoryTemporaryStorage::new(pending_block_number))),
        }
    }
}

impl FromStr for TemporaryStorageKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        match s {
            "inmemory" => Ok(Self::InMemory),
            s => Err(anyhow!("unknown temporary storage kind: \"{}\" - valid values are {:?}", s, Self::VARIANTS)),
        }
    }
}
