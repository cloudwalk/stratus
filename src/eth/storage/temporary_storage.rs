use std::str::FromStr;

use anyhow::anyhow;
use clap::Parser;
use display_json::DebugAsJson;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::Hash;
use crate::eth::primitives::PendingBlock;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionExecution;
use crate::eth::storage::InMemoryTemporaryStorage;

/// Temporary storage (in-between blocks) operations.
pub trait TemporaryStorage: Send + Sync + 'static {
    // -------------------------------------------------------------------------
    // Block number
    // -------------------------------------------------------------------------

    /// Sets the block number being mined.
    fn set_pending_block_number(&self, number: BlockNumber) -> anyhow::Result<()>;

    // Retrieves the block number being mined.
    fn read_pending_block_number(&self) -> anyhow::Result<Option<BlockNumber>>;

    // -------------------------------------------------------------------------
    // Block and executions
    // -------------------------------------------------------------------------

    /// Sets the pending external block being re-executed.
    fn set_pending_external_block(&self, block: ExternalBlock) -> anyhow::Result<()>;

    /// Saves a re-executed transaction to the pending mined block.
    fn save_execution(&self, tx: TransactionExecution, check_conflicts: bool) -> Result<(), StratusError>;

    /// Retrieves the pending transactions of the pending block.
    fn pending_transactions(&self) -> Vec<TransactionExecution>;

    /// Finishes the mining of the pending block and starts a new block.
    fn finish_pending_block(&self) -> anyhow::Result<PendingBlock>;

    /// Retrieves a transaction from the storage.
    fn read_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionExecution>>;

    // -------------------------------------------------------------------------
    // Accounts and slots
    // -------------------------------------------------------------------------

    /// Retrieves an account from the storage. Returns Option when not found.
    fn read_account(&self, address: &Address) -> anyhow::Result<Option<Account>>;

    /// Retrieves an slot from the storage. Returns Option when not found.
    fn read_slot(&self, address: &Address, index: &SlotIndex) -> anyhow::Result<Option<Slot>>;

    // -------------------------------------------------------------------------
    // Global state
    // -------------------------------------------------------------------------

    /// Resets to default empty state.
    fn reset(&self) -> anyhow::Result<()>;
}

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

/// Temporary storage configuration.
#[derive(Parser, DebugAsJson, Clone, serde::Serialize)]
pub struct TemporaryStorageConfig {
    /// Temporary storage implementation.
    #[arg(long = "temp-storage", env = "TEMP_STORAGE")]
    pub temp_storage_kind: TemporaryStorageKind,
}

#[derive(DebugAsJson, Clone, serde::Serialize)]
pub enum TemporaryStorageKind {
    #[serde(rename = "inmemory")]
    InMemory,
}

impl TemporaryStorageConfig {
    /// Initializes temporary storage implementation.
    pub fn init(&self) -> anyhow::Result<Box<dyn TemporaryStorage>> {
        tracing::info!(config = ?self, "creating temporary storage");

        match self.temp_storage_kind {
            TemporaryStorageKind::InMemory => Ok(Box::<InMemoryTemporaryStorage>::default()),
        }
    }
}

impl FromStr for TemporaryStorageKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self, Self::Err> {
        match s {
            "inmemory" => Ok(Self::InMemory),
            s => Err(anyhow!("unknown temporary storage: {}", s)),
        }
    }
}
