use std::sync::Arc;

use anyhow::anyhow;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotSample;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::StorageError;
use crate::eth::storage::TemporaryStorage;
use crate::ext::not;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

#[cfg(feature = "metrics")]
const STORAGE_TEMP: &str = "temporary";
#[cfg(feature = "metrics")]
const STORAGE_PERM: &str = "permanent";
#[cfg(feature = "metrics")]
const DEFAULT_VALUE: &str = "default";

pub struct StratusStorage {
    temp: Arc<dyn TemporaryStorage>,
    perm: Arc<dyn PermanentStorage>,
}

impl StratusStorage {
    /// Creates a new storage with the specified temporary and permanent implementations.
    pub fn new(temp: Arc<dyn TemporaryStorage>, perm: Arc<dyn PermanentStorage>) -> Self {
        Self { temp, perm }
    }

    pub async fn allocate_evm_thread_resources(&self) -> anyhow::Result<()> {
        self.perm.allocate_evm_thread_resources().await?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Block number operations
    // -------------------------------------------------------------------------

    // Retrieves the active block number.
    #[allow(clippy::let_and_return)]
    pub async fn read_active_block_number(&self) -> anyhow::Result<Option<BlockNumber>> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let result = self.temp.read_active_block_number().await;

        #[cfg(feature = "metrics")]
        metrics::inc_storage_read_active_block_number(start.elapsed(), result.is_ok());

        result
    }

    // Retrieves the last mined block number.
    #[allow(clippy::let_and_return)]
    pub async fn read_mined_block_number(&self) -> anyhow::Result<BlockNumber> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let result = self.perm.read_mined_block_number().await;

        #[cfg(feature = "metrics")]
        metrics::inc_storage_read_mined_block_number(start.elapsed(), result.is_ok());

        result
    }

    /// Atomically increments the block number, returning the new value.
    #[allow(clippy::let_and_return)]
    pub async fn increment_block_number(&self) -> anyhow::Result<BlockNumber> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let result = self.perm.increment_block_number().await;

        #[cfg(feature = "metrics")]
        metrics::inc_storage_increment_block_number(start.elapsed(), result.is_ok());

        result
    }

    /// Sets the active block number to a specific value.
    #[allow(clippy::let_and_return)]
    pub async fn set_active_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let result = self.temp.set_active_block_number(number).await;

        #[cfg(feature = "metrics")]
        metrics::inc_storage_set_active_block_number(start.elapsed(), result.is_ok());

        result
    }

    /// Sets the mined block number to a specific value.
    #[allow(clippy::let_and_return)]
    pub async fn set_mined_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let result = self.perm.set_mined_block_number(number).await;

        #[cfg(feature = "metrics")]
        metrics::inc_storage_set_mined_block_number(start.elapsed(), result.is_ok());

        result
    }

    // -------------------------------------------------------------------------
    // State queries
    // -------------------------------------------------------------------------

    /// Retrieves an account from the storage. Returns default value when not found.
    pub async fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Account> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        match self.temp.read_account(address).await? {
            Some(account) => {
                tracing::debug!("account found in the temporary storage");
                #[cfg(feature = "metrics")]
                metrics::inc_storage_read_account(start.elapsed(), STORAGE_TEMP, point_in_time, true);
                Ok(account)
            }
            None => match self.perm.read_account(address, point_in_time).await? {
                Some(account) => {
                    tracing::debug!("account found in the permanent storage");
                    #[cfg(feature = "metrics")]
                    metrics::inc_storage_read_account(start.elapsed(), STORAGE_PERM, point_in_time, true);
                    Ok(account)
                }
                None => {
                    tracing::debug!("account not found, assuming default value");
                    #[cfg(feature = "metrics")]
                    metrics::inc_storage_read_account(start.elapsed(), DEFAULT_VALUE, point_in_time, true);
                    Ok(Account::new_empty(address.clone()))
                }
            },
        }
    }

    /// Retrieves an slot from the storage. Returns default value when not found.
    pub async fn read_slot(&self, address: &Address, index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Slot> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        match self.temp.read_slot(address, index).await? {
            Some(slot) => {
                tracing::debug!("slot found in the temporary storage");
                #[cfg(feature = "metrics")]
                metrics::inc_storage_read_slot(start.elapsed(), STORAGE_TEMP, point_in_time, true);
                Ok(slot)
            }
            None => match self.perm.read_slot(address, index, point_in_time).await? {
                Some(slot) => {
                    tracing::debug!("slot found in the permanent storage");
                    #[cfg(feature = "metrics")]
                    metrics::inc_storage_read_slot(start.elapsed(), STORAGE_PERM, point_in_time, true);
                    Ok(slot)
                }
                None => {
                    tracing::debug!("slot not found, assuming default value");
                    #[cfg(feature = "metrics")]
                    metrics::inc_storage_read_slot(start.elapsed(), DEFAULT_VALUE, point_in_time, true);
                    Ok(Slot::new_empty(index.clone()))
                }
            },
        }
    }

    /// Retrieves multiple slots from the storage. Returns default values when not found.
    pub async fn read_slots(&self, address: &Address, slot_indexes: &[SlotIndex], point_in_time: &StoragePointInTime) -> anyhow::Result<Vec<Slot>> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let mut slots = Vec::with_capacity(slot_indexes.len());
        let mut perm_indexes = Vec::with_capacity(slot_indexes.len());

        // read slots from temporary storage
        for index in slot_indexes {
            match self.temp.read_slot(address, index).await? {
                Some(slot) => {
                    slots.push(slot);
                }
                None => {
                    perm_indexes.push(index.clone());
                }
            }
        }

        // read missing slots from permanent storage
        if not(perm_indexes.is_empty()) {
            let mut perm_slots = self.perm.read_slots(address, &perm_indexes, point_in_time).await?;
            for index in perm_indexes.into_iter() {
                match perm_slots.remove(&index) {
                    Some(value) => slots.push(Slot { index, value }),
                    None => slots.push(Slot::new_empty(index)),
                }
            }
        }

        #[cfg(feature = "metrics")]
        metrics::inc_storage_read_slots(start.elapsed(), point_in_time, true);

        Ok(slots)
    }

    /// Retrieves a block from the storage.
    #[allow(clippy::let_and_return)]
    pub async fn read_block(&self, block_selection: &BlockSelection) -> anyhow::Result<Option<Block>> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let result = self.perm.read_block(block_selection).await;

        #[cfg(feature = "metrics")]
        metrics::inc_storage_read_block(start.elapsed(), result.is_ok());

        result
    }

    /// Retrieves a transaction from the storage.
    #[allow(clippy::let_and_return)]
    pub async fn read_mined_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let result = self.perm.read_mined_transaction(hash).await;

        #[cfg(feature = "metrics")]
        metrics::inc_storage_read_mined_transaction(start.elapsed(), result.is_ok());

        result
    }

    /// Retrieves logs from the storage.
    #[allow(clippy::let_and_return)]
    pub async fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let result = self.perm.read_logs(filter).await;

        #[cfg(feature = "metrics")]
        metrics::inc_storage_read_logs(start.elapsed(), result.is_ok());

        result
    }

    // -------------------------------------------------------------------------
    // State mutations
    // -------------------------------------------------------------------------

    /// Persists accounts like pre-genesis accounts or test accounts.
    #[allow(clippy::let_and_return)]
    pub async fn save_accounts_to_perm(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let result = self.perm.save_accounts(accounts).await;

        #[cfg(feature = "metrics")]
        metrics::inc_storage_save_accounts(start.elapsed(), result.is_ok());

        result
    }

    /// Temporarily saves account's changes generated during block production.
    #[allow(clippy::let_and_return)]
    pub async fn save_account_changes_to_temp(&self, changes: Vec<ExecutionAccountChanges>) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let result = self.temp.save_account_changes(changes).await;

        #[cfg(feature = "metrics")]
        metrics::inc_storage_save_account_changes(start.elapsed(), result.is_ok());

        result
    }

    /// If necessary, flushes temporary state to durable storage.
    #[allow(clippy::let_and_return)]
    pub async fn flush_temp(&self) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let result = self.temp.flush().await;

        #[cfg(feature = "metrics")]
        metrics::inc_storage_flush_temp(start.elapsed(), result.is_ok());

        result
    }

    /// Commits changes to permanent storage and prepares temporary storage for a new block to be produced.
    pub async fn commit_to_perm(&self, block: Block) -> anyhow::Result<(), StorageError> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();
        #[cfg(feature = "metrics")]
        let label_size_by_tx = block.label_size_by_transactions();
        #[cfg(feature = "metrics")]
        let label_size_by_gas = block.label_size_by_gas();
        #[cfg(feature = "metrics")]
        let gas_used = block.header.gas_used.as_u64();

        // save block to permanent storage and clears temporary storage
        let next_number = block.number().next();
        let result = self.perm.save_block(block).await;
        self.reset_temp().await?;
        self.set_active_block_number(next_number).await?;

        #[cfg(feature = "metrics")]
        metrics::inc_storage_commit(start.elapsed(), label_size_by_tx, label_size_by_gas, result.is_ok());
        #[cfg(feature = "metrics")]
        metrics::inc_n_storage_gas_total(gas_used);

        result
    }

    /// Resets temporary storage.
    #[allow(clippy::let_and_return)]
    pub async fn reset_temp(&self) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let result = self.temp.reset().await;

        #[cfg(feature = "metrics")]
        metrics::inc_storage_reset(start.elapsed(), STORAGE_TEMP, result.is_ok());

        result
    }

    /// Resets permanent storage down to specific block_number.
    #[allow(clippy::let_and_return)]
    pub async fn reset_perm(&self, block_number: BlockNumber) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let result = self.perm.reset_at(block_number).await;

        #[cfg(feature = "metrics")]
        metrics::inc_storage_reset(start.elapsed(), STORAGE_PERM, result.is_ok());

        result
    }

    // -------------------------------------------------------------------------
    // Utils
    // -------------------------------------------------------------------------

    /// Translates a block selection to a specific storage point-in-time indicator.
    pub async fn translate_to_point_in_time(&self, block_selection: &BlockSelection) -> anyhow::Result<StoragePointInTime> {
        match block_selection {
            BlockSelection::Latest => Ok(StoragePointInTime::Present),
            BlockSelection::Number(number) => {
                let current_block = self.perm.read_mined_block_number().await?;
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

    pub async fn read_slots_sample(&self, start: BlockNumber, end: BlockNumber, max_samples: u64, seed: u64) -> anyhow::Result<Vec<SlotSample>> {
        self.perm.read_slots_sample(start, end, max_samples, seed).await
    }
}
