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
            None => self.perm.maybe_read_account(address, point_in_time).await?,
        };

        Ok(account)
    }

    /// Retrieves an slot from the storage. Returns Option when not found.
    async fn maybe_read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        let slot = match self.temp.maybe_read_slot(address, slot_index, point_in_time).await? {
            Some(slot) => Some(slot),
            None => self.perm.maybe_read_slot(address, slot_index, point_in_time).await?,
        };

        Ok(slot)
    }

    /// Retrieves a block from the storage.
    async fn read_block(&self, block_selection: &BlockSelection) -> anyhow::Result<Option<Block>> {
        let block = match self.temp.read_block(block_selection).await? {
            Some(block) => Some(block),
            None => self.perm.read_block(block_selection).await?,
        };

        Ok(block)
    }

    /// Retrieves a transaction from the storage.
    async fn read_mined_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        let tx = match self.temp.read_mined_transaction(hash).await? {
            Some(tx) => Some(tx),
            None => self.perm.read_mined_transaction(hash).await?,
        };

        Ok(tx)
    }

    /// Retrieves logs from the storage.
    async fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        let mut logs = self.temp.read_logs(filter).await?;

        if logs.is_empty() {
            logs = self.perm.read_logs(filter).await?;
        };

        Ok(logs)
    }

    /// Persist atomically all changes from a block.
    async fn save_block(&self, block: Block) -> anyhow::Result<(), EthStorageError> {
        self.temp.save_block(block.clone()).await?;
        self.perm.save_block(block).await?;

        Ok(())
    }

    /// Temporarily stores account changes during block production
    async fn save_account_changes(&self, block_number: BlockNumber, execution: Execution) -> anyhow::Result<()> {
        self.temp.save_account_changes(block_number, execution).await
    }

    /// Resets all state to a specific block number.
    async fn reset(&self, number: BlockNumber) -> anyhow::Result<()> {
        self.temp.reset(number).await?;
        self.perm.reset(number).await?;

        Ok(())
    }

    /// Enables genesis block.
    ///
    /// TODO: maybe can use save_block from a default method.
    async fn enable_genesis(&self, _genesis: Block) -> anyhow::Result<()> {
        todo!()
    }

    async fn save_initial_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        self.temp.save_initial_accounts(accounts.clone()).await?;
        self.perm.save_initial_accounts(accounts).await?;

        Ok(())
    }
}
