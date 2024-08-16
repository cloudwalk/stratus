use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use anyhow::bail;

use super::rocks_state::RocksStorageState;
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
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::StoragePointInTime;

#[derive(Debug)]
pub struct RocksPermanentStorage {
    pub state: RocksStorageState,
    block_number: AtomicU64,
}

impl RocksPermanentStorage {
    pub fn new(rocks_path_prefix: Option<String>) -> anyhow::Result<Self> {
        tracing::info!("setting up rocksdb storage");
        let path = if let Some(prefix) = rocks_path_prefix {
            // run some checks on the given prefix
            if prefix.is_empty() {
                bail!("given prefix for RocksDB is empty, try not providing the flag");
            }

            if Path::new(&prefix).is_dir() || Path::new(&prefix).iter().count() > 1 {
                tracing::warn!(?prefix, "given prefix for RocksDB might put it in another folder");
            }

            let path = format!("{prefix}-rocksdb");
            tracing::info!("starting rocksdb storage - at custom path: '{:?}'", path);
            path
        } else {
            tracing::info!("starting rocksdb storage - at default path: 'data/rocksdb'");
            "data/rocksdb".to_string()
        };

        let state = RocksStorageState::new(path)?;
        let block_number = state.preload_block_number()?;
        Ok(Self { state, block_number })
    }

    // -------------------------------------------------------------------------
    // State methods
    // -------------------------------------------------------------------------

    pub fn clear(&self) -> anyhow::Result<()> {
        self.state.clear()?;
        self.block_number.store(0, Ordering::SeqCst);
        Ok(())
    }

    pub fn revert_state_to_block(&self, block_number: BlockNumber) -> anyhow::Result<()> {
        self.state.revert_state_to_block(block_number.into())
    }
}

impl PermanentStorage for RocksPermanentStorage {
    // -------------------------------------------------------------------------
    // Block number operations
    // -------------------------------------------------------------------------

    fn read_mined_block_number(&self) -> anyhow::Result<BlockNumber> {
        Ok(self.block_number.load(Ordering::SeqCst).into())
    }

    fn set_mined_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        self.block_number.store(number.as_u64(), Ordering::SeqCst);
        Ok(())
    }

    // -------------------------------------------------------------------------
    // State operations
    // ------------------------------------------------------------------------

    fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>> {
        self.state.read_account(address, point_in_time)
    }

    fn read_slot(&self, address: &Address, index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        self.state.read_slot(address, index, point_in_time)
    }

    fn read_block(&self, selection: &BlockFilter) -> anyhow::Result<Option<Block>> {
        let block = self.state.read_block(selection);
        if let Ok(Some(block)) = &block {
            tracing::trace!(?selection, ?block, "block found");
        }
        block
    }

    fn read_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        self.state.read_transaction(hash)
    }

    fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        self.state.read_logs(filter)
    }

    fn save_block(&self, block: Block) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        {
            self.state.export_metrics()?;
        }
        self.state.save_block(block)
    }

    fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        self.state.save_accounts(accounts)
    }

    #[cfg(feature = "dev")]
    fn reset(&self) -> anyhow::Result<()> {
        self.block_number.store(0u64, Ordering::SeqCst);
        self.state.reset()
    }
}
