// TODO: doc
use std::sync::Arc;

use async_trait::async_trait;

use super::EthStorageError;
use super::InMemoryStorage;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Execution;
use crate::eth::primitives::ExecutionConflicts;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::EthStorage;

pub struct StratusStorage {
    temp: InMemoryStorage,
    perm: Arc<dyn EthStorage>,
}

#[async_trait]
impl EthStorage for StratusStorage {
    // -------------------------------------------------------------------------
    // Block number operations
    // -------------------------------------------------------------------------

    // Retrieves the last mined block number.
    async fn read_current_block_number(&self) -> anyhow::Result<BlockNumber> {
        self.temp.read_current_block_number().await
    }

    /// Atomically increments the block number, returning the new value.
    async fn increment_block_number(&self) -> anyhow::Result<BlockNumber> {
        self.temp.increment_block_number().await
    }

    // -------------------------------------------------------------------------
    // State operations
    // -------------------------------------------------------------------------

    /// Checks if the transaction execution conflicts with the current storage state.
    async fn check_conflicts(&self, execution: &Execution) -> anyhow::Result<Option<ExecutionConflicts>> {
        self.temp.check_conflicts(execution).await
    }

    /// Retrieves an account from the storage. Returns Option when not found.
    async fn maybe_read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>> {
        let account = match self.temp.maybe_read_account(address, point_in_time).await? {
            Some(account) => Some(account),
            None => {
                match self.perm.maybe_read_account(address, point_in_time).await? {
                    Some(account) => Some(account),
                    None => None
                }
            },
        };

        Ok(account)
    }

    /// Retrieves an slot from the storage. Returns Option when not found.
    async fn maybe_read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        let slot = match self.temp.maybe_read_slot(address, slot_index, point_in_time).await? {
            Some(slot) => Some(slot),
            None => {
                match self.perm.maybe_read_slot(address, slot_index, point_in_time).await? {
                    Some(slot) => Some(slot),
                    None => None
                }
            }
        };

        Ok(slot)
    }

    /// Retrieves a block from the storage.
    async fn read_block(&self, block_selection: &BlockSelection) -> anyhow::Result<Option<Block>> {
        let block = match self.temp.read_block(block_selection).await? {
            Some(block) => Some(block),
            None => {
                match self.perm.read_block(block_selection).await? {
                    Some(block) => Some(block),
                    None => None
                }
            }
        };

        Ok(block)
    }

    /// Retrieves a transaction from the storage.
    async fn read_mined_transaction(&self, _hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        todo!()
    }

    /// Retrieves logs from the storage.
    async fn read_logs(&self, _filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        todo!()
    }

    /// Persist atomically all changes from a block.
    async fn save_block(&self, _block: Block) -> anyhow::Result<(), EthStorageError> {
        todo!()
    }

    /// Temporarily stores account changes during block production
    async fn save_account_changes(&self, _block_number: BlockNumber, _execution: Execution) -> anyhow::Result<()> {
        todo!()
    }

    /// Resets all state to a specific block number.
    async fn reset(&self, _number: BlockNumber) -> anyhow::Result<()> {
        todo!()
    }

    /// Enables genesis block.
    ///
    /// TODO: maybe can use save_block from a default method.
    async fn enable_genesis(&self, _genesis: Block) -> anyhow::Result<()> {
        todo!()
    }

    /// Enables test accounts.
    ///
    /// TODO: maybe can use save_accounts from a default method.
    async fn enable_test_accounts(&self, _test_accounts: Vec<Account>) -> anyhow::Result<()> {
        todo!()
    }
}
