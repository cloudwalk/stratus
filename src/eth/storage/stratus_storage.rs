use std::sync::Arc;
use std::time::Instant;

use anyhow::anyhow;
use async_trait::async_trait;

use super::permanent_storage::PermanentStorage;
use super::temporary_storage::TemporaryStorage;
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
use crate::infra::metrics;

pub struct StratusStorage {
    temp: InMemoryStorage,
    perm: Arc<dyn PermanentStorage>,
}

#[allow(dead_code)]
impl StratusStorage {
    pub fn new(temp: InMemoryStorage, perm: Arc<dyn PermanentStorage>) -> Self {
        Self { temp, perm }
    }

    /// Retrieves an account from the storage. Returns default value when not found.
    pub async fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Account> {
        let start = Instant::now();
        let result = match TemporaryStorage::maybe_read_account(self, address, point_in_time).await? {
            Some(account) => Ok(account),
            None => match PermanentStorage::maybe_read_account(self, address, point_in_time).await? {
                Some(account) => Ok(account),
                None => Ok(Account {
                    address: address.clone(),
                    ..Account::default()
                }),
            },
        };

        metrics::inc_storage_read_account(start.elapsed(), point_in_time, result.is_ok());
        result
    }

    /// Retrieves an slot from the storage. Returns default value when not found.
    pub async fn read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Slot> {
        let start = Instant::now();
        let result = match TemporaryStorage::maybe_read_slot(&self.temp, address, slot_index, point_in_time).await? {
            Some(slot) => Ok(slot),
            None => match PermanentStorage::maybe_read_slot(&*self.perm, address, slot_index, point_in_time).await? {
                Some(slot) => Ok(slot),
                None => Ok(Slot {
                    index: slot_index.clone(),
                    ..Default::default()
                }),
            },
        };
        metrics::inc_storage_read_slot(start.elapsed(), point_in_time, result.is_ok());
        result
    }

    /// Translates a block selection to a specific storage point-in-time indicator.
    pub async fn translate_to_point_in_time(&self, block_selection: &BlockSelection) -> anyhow::Result<StoragePointInTime> {
        match block_selection {
            BlockSelection::Latest => Ok(StoragePointInTime::Present),
            BlockSelection::Number(number) => {
                let current_block = TemporaryStorage::read_current_block_number(&self.temp).await?;
                if number <= &current_block {
                    Ok(StoragePointInTime::Past(*number))
                } else {
                    Ok(StoragePointInTime::Past(current_block))
                }
            }
            BlockSelection::Earliest | BlockSelection::Hash(_) => match self.read_block(block_selection).await? {
                Some(block) => Ok(StoragePointInTime::Past(block.header.number)),
                None => Err(anyhow!(
                    "failed to select block because it is greater than current block number or block hash is invalid."
                )),
            },
        }
    }

    pub async fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        self.perm.save_accounts(accounts).await
    }
}

#[async_trait]
impl TemporaryStorage for StratusStorage {
    // Retrieves the last mined block number.
    async fn read_current_block_number(&self) -> anyhow::Result<BlockNumber> {
        let start = Instant::now();
        let result = TemporaryStorage::read_current_block_number(&self.temp).await;
        metrics::inc_storage_read_current_block_number(start.elapsed(), result.is_ok());
        result
    }

    /// Atomically increments the block number, returning the new value.
    async fn increment_block_number(&self) -> anyhow::Result<BlockNumber> {
        let start = Instant::now();
        let result = TemporaryStorage::increment_block_number(&self.temp).await;
        metrics::inc_storage_increment_block_number(start.elapsed(), result.is_ok());
        result
    }

    /// Checks if the transaction execution conflicts with the current storage state.
    async fn check_conflicts(&self, execution: &Execution) -> anyhow::Result<Option<ExecutionConflicts>> {
        let start = Instant::now();
        let result = TemporaryStorage::check_conflicts(&self.temp, execution).await;
        metrics::inc_storage_check_conflicts(start.elapsed(), result.as_ref().is_ok_and(|v| v.is_some()), result.is_ok());
        result
    }

    /// Retrieves an account from the storage. Returns Option when not found.
    async fn maybe_read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>> {
        let start = Instant::now();
        let result = TemporaryStorage::maybe_read_account(&self.temp, address, point_in_time).await;
        metrics::inc_storage_maybe_read_account(start.elapsed(), point_in_time, result.as_ref().is_ok_and(|v| v.is_some()), result.is_ok());
        result
    }

    /// Retrieves an slot from the storage. Returns Option when not found.
    async fn maybe_read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        let start = Instant::now();
        let result = TemporaryStorage::maybe_read_slot(&self.temp, address, slot_index, point_in_time).await;
        metrics::inc_storage_maybe_read_slot(start.elapsed(), point_in_time, result.as_ref().is_ok_and(|v| v.is_some()), result.is_ok());
        result
    }

    /// Commits changes to permanent storage and flushes temporary storage
    /// Basically calls the `save_block` method from the permanent storage, which
    /// will by definition update accounts, slots, transactions, logs etc
    async fn commit(&self, block: Block) -> anyhow::Result<(), EthStorageError> {
        let start = Instant::now();
        let result = self.perm.save_block(block).await;

        // clears temporary storage
        self.temp.flush().await;
        
        metrics::inc_storage_commit(start.elapsed(), result.is_ok());
        result
    }

    /// Persist atomically all changes from a block.
    async fn save_block(&self, block: Block) -> anyhow::Result<(), EthStorageError> {
        let start = Instant::now();
        let result = TemporaryStorage::save_block(&self.temp, block).await;
        metrics::inc_storage_save_block(start.elapsed(), result.is_ok());
        result
    }

    /// Temporarily stores account changes during block production
    async fn save_account_changes(&self, block_number: BlockNumber, execution: Execution) -> anyhow::Result<()> {
        let start = Instant::now();
        let result = TemporaryStorage::save_account_changes(&self.temp, block_number, execution).await;
        metrics::inc_storage_save_account_changes(start.elapsed(), result.is_ok());
        result
    }

    /// Resets all state to a specific block number.
    async fn reset(&self) -> anyhow::Result<()> {
        let start = Instant::now();
        let result = TemporaryStorage::reset(&self.temp).await;
        metrics::inc_storage_reset(start.elapsed(), result.is_ok());
        result
    }
}

#[async_trait]
impl PermanentStorage for StratusStorage {
    // -------------------------------------------------------------------------
    // Block number operations
    // -------------------------------------------------------------------------

    // Retrieves the last mined block number.
    async fn read_current_block_number(&self) -> anyhow::Result<BlockNumber> {
        let start = Instant::now();
        let result = self.perm.read_current_block_number().await;
        metrics::inc_storage_read_current_block_number(start.elapsed(), result.is_ok());
        result
    }

    // -------------------------------------------------------------------------
    // State operations
    // -------------------------------------------------------------------------

    /// Checks if the transaction execution conflicts with the current storage state.
    async fn check_conflicts(&self, execution: &Execution) -> anyhow::Result<Option<ExecutionConflicts>> {
        let start = Instant::now();
        let result = self.perm.check_conflicts(execution).await;
        metrics::inc_storage_check_conflicts(start.elapsed(), result.as_ref().is_ok_and(|v| v.is_some()), result.is_ok());
        result
    }

    /// Retrieves an account from the storage. Returns Option when not found.
    async fn maybe_read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>> {
        let start = Instant::now();
        let result = self.perm.maybe_read_account(address, point_in_time).await;
        metrics::inc_storage_maybe_read_account(start.elapsed(), point_in_time, result.as_ref().is_ok_and(|v| v.is_some()), result.is_ok());
        result
    }

    /// Retrieves an slot from the storage. Returns Option when not found.
    async fn maybe_read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        let start = Instant::now();
        let result = self.perm.maybe_read_slot(address, slot_index, point_in_time).await;
        metrics::inc_storage_maybe_read_slot(start.elapsed(), point_in_time, result.as_ref().is_ok_and(|v| v.is_some()), result.is_ok());
        result
    }

    /// Retrieves a block from the storage.
    async fn read_block(&self, block_selection: &BlockSelection) -> anyhow::Result<Option<Block>> {
        let start = Instant::now();
        let result = self.perm.read_block(block_selection).await;
        metrics::inc_storage_read_block(start.elapsed(), result.is_ok());
        result
    }

    /// Retrieves a transaction from the storage.
    async fn read_mined_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        let start = Instant::now();
        let result = self.perm.read_mined_transaction(hash).await;
        metrics::inc_storage_read_mined_transaction(start.elapsed(), result.is_ok());
        result
    }

    /// Retrieves logs from the storage.
    async fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        let start = Instant::now();
        let result = self.perm.read_logs(filter).await;
        metrics::inc_storage_read_logs(start.elapsed(), result.is_ok());
        result
    }

    /// Persist atomically all changes from a block.
    async fn save_block(&self, block: Block) -> anyhow::Result<(), EthStorageError> {
        let start = Instant::now();
        let result = self.perm.save_block(block).await;
        metrics::inc_storage_save_block(start.elapsed(), result.is_ok());
        result
    }

    /// Temporarily stores account changes during block production
    async fn save_account_changes(&self, block_number: BlockNumber, execution: Execution) -> anyhow::Result<()> {
        let start = Instant::now();
        let result = self.perm.save_account_changes(block_number, execution).await;
        metrics::inc_storage_save_account_changes(start.elapsed(), result.is_ok());
        result
    }

    /// Resets all state to a specific block number.
    async fn reset(&self, number: BlockNumber) -> anyhow::Result<()> {
        let start = Instant::now();
        let result = self.perm.reset(number).await;
        metrics::inc_storage_reset(start.elapsed(), result.is_ok());
        result
    }

    /// Enables genesis block.
    ///
    /// TODO: maybe can use save_block from a default method.
    async fn enable_genesis(&self, genesis: Block) -> anyhow::Result<()> {
        self.perm.enable_genesis(genesis).await
    }

    /// Enables pre-genesis accounts
    async fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        let start = Instant::now();
        let result = self.perm.save_accounts(accounts).await;
        metrics::inc_storage_save_accounts(start.elapsed(), result.is_ok());
        result
    }
}
