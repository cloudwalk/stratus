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
    _temp: InMemoryStorage,
    _perm: Arc<dyn EthStorage>,
}

#[async_trait]
impl EthStorage for StratusStorage {
    // -------------------------------------------------------------------------
    // Block number operations
    // -------------------------------------------------------------------------

    // Retrieves the last mined block number.
    async fn read_current_block_number(&self) -> anyhow::Result<BlockNumber> {
        todo!()
    }

    /// Atomically increments the block number, returning the new value.
    async fn increment_block_number(&self) -> anyhow::Result<BlockNumber> {
        todo!()
    }

    // -------------------------------------------------------------------------
    // State operations
    // -------------------------------------------------------------------------

    /// Checks if the transaction execution conflicts with the current storage state.
    async fn check_conflicts(&self, _execution: &Execution) -> anyhow::Result<Option<ExecutionConflicts>> {
        todo!()
    }

    /// Retrieves an account from the storage. Returns Option when not found.
    async fn maybe_read_account(&self, _address: &Address, _point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>> {
        todo!()
    }

    /// Retrieves an slot from the storage. Returns Option when not found.
    async fn maybe_read_slot(&self, _address: &Address, _slot_index: &SlotIndex, _point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        todo!()
    }

    /// Retrieves a block from the storage.
    async fn read_block(&self, _block_selection: &BlockSelection) -> anyhow::Result<Option<Block>> {
        todo!()
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
