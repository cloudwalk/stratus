use std::sync::Arc;
use std::time::Instant;

use anyhow::anyhow;

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
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::StorageError;
use crate::eth::storage::TemporaryStorage;
use crate::infra::metrics;

pub struct StratusStorage {
    temp: Arc<dyn TemporaryStorage>,
    perm: Arc<dyn PermanentStorage>,
}

impl StratusStorage {
    pub fn new(temp: Arc<dyn TemporaryStorage>, perm: Arc<dyn PermanentStorage>) -> Self {
        Self { temp, perm }
    }

    // -------------------------------------------------------------------------
    // Block number operations
    // -------------------------------------------------------------------------

    // Retrieves the last mined block number.
    pub async fn read_current_block_number(&self) -> anyhow::Result<BlockNumber> {
        let start = Instant::now();
        let result = self.perm.read_current_block_number().await;
        metrics::inc_storage_read_current_block_number(start.elapsed(), result.is_ok());
        result
    }

    /// Atomically increments the block number, returning the new value.
    pub async fn increment_block_number(&self) -> anyhow::Result<BlockNumber> {
        let start = Instant::now();
        let result = self.perm.increment_block_number().await;
        metrics::inc_storage_increment_block_number(start.elapsed(), result.is_ok());
        result
    }

    // -------------------------------------------------------------------------
    // State queries
    // -------------------------------------------------------------------------

    /// Checks if the transaction execution conflicts with the current storage state.
    pub async fn check_conflicts(&self, execution: &Execution) -> anyhow::Result<Option<ExecutionConflicts>> {
        let start = Instant::now();
        let result = self.temp.check_conflicts(execution).await;
        metrics::inc_storage_check_conflicts(start.elapsed(), result.as_ref().is_ok_and(|v| v.is_some()), result.is_ok());
        result
    }

    /// Retrieves an account from the storage. Returns default value when not found.
    pub async fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Account> {
        let start = Instant::now();
        let result = match self.temp.maybe_read_account(address, point_in_time).await? {
            Some(account) => {
                tracing::debug!("account found in the temporary storage");
                metrics::inc_storage_read_account(start.elapsed(), point_in_time, true, metrics::StorageType::Temp);
                Ok(account)
            }
            None => match self.perm.maybe_read_account(address, point_in_time).await? {
                Some(account) => {
                    tracing::debug!("account found in the permanent storage");
                    metrics::inc_storage_read_account(start.elapsed(), point_in_time, true, metrics::StorageType::Perm);
                    Ok(account)
                }
                None => {
                    tracing::debug!("account not found, assuming default value");
                    metrics::inc_storage_read_account(start.elapsed(), point_in_time, true, metrics::StorageType::None);
                    Ok(Account {
                        address: address.clone(),
                        ..Account::default()
                    })
                }
            },
        };
        result
    }

    /// Retrieves an slot from the storage. Returns default value when not found.
    pub async fn read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Slot> {
        let start = Instant::now();
        let result = match self.temp.maybe_read_slot(address, slot_index, point_in_time).await? {
            Some(slot) => {
                tracing::debug!("slot found in the temporary storage");
                metrics::inc_storage_read_slot(start.elapsed(), point_in_time, true, metrics::StorageType::Temp);
                Ok(slot)
            }
            None => match self.perm.maybe_read_slot(address, slot_index, point_in_time).await? {
                Some(slot) => {
                    tracing::debug!("slot found in the permanent storage");
                    metrics::inc_storage_read_slot(start.elapsed(), point_in_time, true, metrics::StorageType::Perm);
                    Ok(slot)
                }
                None => {
                    tracing::debug!("slot not found, assuming default value");
                    metrics::inc_storage_read_slot(start.elapsed(), point_in_time, true, metrics::StorageType::None);
                    Ok(Slot {
                        index: slot_index.clone(),
                        ..Default::default()
                    })
                }
            },
        };
        result
    }

    /// Retrieves a block from the storage.
    pub async fn read_block(&self, block_selection: &BlockSelection) -> anyhow::Result<Option<Block>> {
        let start = Instant::now();
        let result = self.perm.read_block(block_selection).await;
        metrics::inc_storage_read_block(start.elapsed(), result.is_ok());
        result
    }

    /// Retrieves a transaction from the storage.
    pub async fn read_mined_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        let start = Instant::now();
        let result = self.perm.read_mined_transaction(hash).await;
        metrics::inc_storage_read_mined_transaction(start.elapsed(), result.is_ok());
        result
    }

    /// Retrieves logs from the storage.
    pub async fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        let start = Instant::now();
        let result = self.perm.read_logs(filter).await;
        metrics::inc_storage_read_logs(start.elapsed(), result.is_ok());
        result
    }

    // -------------------------------------------------------------------------
    // State mutations
    // -------------------------------------------------------------------------

    /// Persist accounts like pre-genesis accounts or test accounts.
    pub async fn save_accounts_to_perm(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        let start = Instant::now();
        let result = self.perm.save_accounts(accounts).await;
        metrics::inc_storage_save_accounts(start.elapsed(), result.is_ok());
        result
    }

    /// Persists temporary accounts changes produced during block production.
    pub async fn save_account_changes_to_temp(&self, block_number: BlockNumber, execution: Execution) -> anyhow::Result<()> {
        let start = Instant::now();
        let result = self.temp.save_account_changes(block_number, execution).await;
        metrics::inc_storage_save_account_changes(start.elapsed(), result.is_ok());
        result
    }

    /// Commits changes to permanent storage and prepares temporary storage to a new block to be produced.
    pub async fn commit_to_perm(&self, block: Block) -> anyhow::Result<(), StorageError> {
        let start = Instant::now();

        let result = self.perm.save_block(block).await;
        self.reset_temp().await?;

        metrics::inc_storage_commit(start.elapsed(), result.is_ok());
        result
    }

    /// Resets temporary storage
    pub async fn reset_temp(&self) -> anyhow::Result<()> {
        let start = Instant::now();
        let result = self.temp.reset().await;
        metrics::inc_storage_reset(start.elapsed(), result.is_ok());
        result
    }

    /// Resets permanent storage down to specific block_number
    pub async fn reset_perm(&self, block_number: BlockNumber) -> anyhow::Result<()> {
        let start = Instant::now();
        let result = self.perm.reset_at(block_number).await;
        metrics::inc_storage_reset(start.elapsed(), result.is_ok());
        result
    }

    // -------------------------------------------------------------------------
    // Utils
    // -------------------------------------------------------------------------

    /// Enables genesis block.
    ///
    /// TODO: maybe can use save_block from a default method.
    pub async fn enable_genesis(&self, genesis: Block) -> anyhow::Result<()> {
        self.perm.enable_genesis(genesis).await
    }

    /// Translates a block selection to a specific storage point-in-time indicator.
    pub async fn translate_to_point_in_time(&self, block_selection: &BlockSelection) -> anyhow::Result<StoragePointInTime> {
        match block_selection {
            BlockSelection::Latest => Ok(StoragePointInTime::Present),
            BlockSelection::Number(number) => {
                let current_block = self.perm.read_current_block_number().await?;
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
}
