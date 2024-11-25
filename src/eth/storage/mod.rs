//! Ethereum / EVM storage.

pub use permanent::InMemoryPermanentStorage;
pub use permanent::PermanentStorage;
pub use permanent::PermanentStorageConfig;
pub use permanent::PermanentStorageKind;
pub use stratus_storage::StratusStorage;
pub use temporary::InMemoryTemporaryStorage;
pub use temporary::TemporaryStorage;
pub use temporary::TemporaryStorageConfig;
pub use temporary::TemporaryStorageKind;

pub mod permanent;
mod stratus_storage;
mod temporary;

use std::sync::Arc;

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
use crate::eth::primitives::PendingBlock;
use crate::eth::primitives::PendingBlockHeader;
use crate::eth::primitives::PointInTime;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionStage;

pub trait Storage: Sized {
    // -------------------------------------------------------------------------
    // Block number
    // -------------------------------------------------------------------------

    fn read_block_number_to_resume_import(&self) -> Result<BlockNumber, StratusError>;

    fn read_pending_block_header(&self) -> Result<Option<PendingBlockHeader>, StratusError>;

    fn read_mined_block_number(&self) -> Result<BlockNumber, StratusError>;

    fn set_pending_block_number(&self, block_number: BlockNumber) -> Result<(), StratusError>;

    fn set_pending_block_number_as_next(&self) -> Result<(), StratusError>;

    fn set_pending_block_number_as_next_if_not_set(&self) -> Result<(), StratusError>;

    fn set_mined_block_number(&self, block_number: BlockNumber) -> Result<(), StratusError>;

    // -------------------------------------------------------------------------
    // Accounts and slots
    // -------------------------------------------------------------------------

    fn save_accounts(&self, accounts: Vec<Account>) -> Result<(), StratusError>;

    fn read_account(&self, address: Address, point_in_time: PointInTime) -> Result<Account, StratusError>;

    fn read_slot(&self, address: Address, index: SlotIndex, point_in_time: PointInTime) -> Result<Slot, StratusError>;

    // -------------------------------------------------------------------------
    // Blocks
    // -------------------------------------------------------------------------

    fn save_execution(&self, tx: TransactionExecution, check_conflicts: bool) -> Result<(), StratusError>;

    /// Retrieves pending transactions being mined.
    fn pending_transactions(&self) -> Vec<TransactionExecution>;

    fn finish_pending_block(&self) -> Result<PendingBlock, StratusError>;

    fn save_block(&self, block: Block) -> Result<(), StratusError>;

    fn save_block_batch(&self, blocks: Vec<Block>) -> Result<(), StratusError> {
        blocks.into_iter().try_for_each(|block| self.save_block(block))
    }

    fn read_block(&self, filter: BlockFilter) -> Result<Option<Block>, StratusError>;

    fn read_transaction(&self, tx_hash: Hash) -> Result<Option<TransactionStage>, StratusError>;

    fn read_logs(&self, filter: &LogFilter) -> Result<Vec<LogMined>, StratusError>;

    #[cfg(feature = "dev")]
    /// Resets the storage to the genesis state used in dev-mode.
    ///
    /// TODO: For now it uses the dev genesis block and test accounts, but it should be refactored to support genesis.json files.
    fn reset_to_genesis(&self) -> Result<(), StratusError>;

    /// Translates a block filter to a specific storage point-in-time indicator.
    fn translate_to_point_in_time(&self, block_filter: BlockFilter) -> Result<PointInTime, StratusError>;
}

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

/// Configuration that can be used by any binary that interacts with Stratus storage.
#[derive(Parser, DebugAsJson, Clone, serde::Serialize)]
pub struct StratusStorageConfig {
    #[clap(flatten)]
    pub temp_storage: TemporaryStorageConfig,

    #[clap(flatten)]
    pub perm_storage: PermanentStorageConfig,
}

impl StratusStorageConfig {
    /// Initializes Stratus storage.
    pub fn init(&self) -> Result<Arc<StratusStorage>, StratusError> {
        let temp_storage = self.temp_storage.init()?;
        let perm_storage = self.perm_storage.init()?;
        let storage = StratusStorage::new(temp_storage, perm_storage)?;

        Ok(Arc::new(storage))
    }
}
