use std::sync::Arc;

use anyhow::anyhow;
use tracing::Span;

use crate::config::PermanentStorageKind;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::ExecutionConflicts;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::PendingBlock;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotIndexes;
use crate::eth::primitives::SlotSample;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::TemporaryStorage;
use crate::ext::not;
use crate::ext::SpanExt;
#[cfg(feature = "metrics")]
use crate::infra::metrics;

#[cfg(feature = "metrics")]
mod label {
    pub(super) const TEMP: &str = "temporary";
    pub(super) const PERM: &str = "permanent";
    pub(super) const DEFAULT: &str = "default";
}

/// Proxy that simplifies interaction with permanent and temporary storages.
///
/// Additionaly it tracks metrics that are independent of the storage implementation.
pub struct StratusStorage {
    temp: Arc<dyn TemporaryStorage>,
    perm: Arc<dyn PermanentStorage>,
}

impl StratusStorage {
    // -------------------------------------------------------------------------
    // Initialization
    // -------------------------------------------------------------------------

    /// Creates a new storage with the specified temporary and permanent implementations.
    pub fn new(temp: Arc<dyn TemporaryStorage>, perm: Arc<dyn PermanentStorage>) -> Self {
        Self { temp, perm }
    }

    pub async fn allocate_evm_thread_resources(&self) -> anyhow::Result<()> {
        self.perm.allocate_evm_thread_resources().await?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Metadata
    // -------------------------------------------------------------------------
    pub fn perm_kind(&self) -> PermanentStorageKind {
        self.perm.kind()
    }

    // -------------------------------------------------------------------------
    // Block number
    // -------------------------------------------------------------------------

    #[tracing::instrument(skip_all)]
    pub async fn read_block_number_to_resume_import(&self) -> anyhow::Result<BlockNumber> {
        // if does not have the zero block present, should resume from zero
        let zero = self.read_block(&BlockSelection::Number(BlockNumber::ZERO)).await?;
        if zero.is_none() {
            tracing::info!(number = %0, reason = %"block ZERO does not exist", "resume from ZERO");
            return Ok(BlockNumber::ZERO);
        }

        // try to resume from active block number
        let active_number = self.read_active_block_number().await?;
        if let Some(active_number) = active_number {
            tracing::info!(number = %active_number, reason = %"set in storage", "resume from ACTIVE");
            return Ok(active_number);
        }

        // fallback to last mined block number
        let mined_number = self.read_mined_block_number().await?;
        let mined_block = self.read_block(&BlockSelection::Number(mined_number)).await?;
        match mined_block {
            Some(_) => {
                tracing::info!(number = %mined_number, reason = %"set in storage and block exist", "resume from MINED + 1");
                Ok(mined_number.next())
            }
            None => {
                tracing::info!(number = %mined_number, reason = %"set in storage but block does not exist", "resume from MINED");
                Ok(mined_number)
            }
        }
    }

    #[tracing::instrument(skip_all)]
    pub async fn read_active_block_number(&self) -> anyhow::Result<Option<BlockNumber>> {
        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.temp.read_active_block_number().await;
            metrics::inc_storage_read_active_block_number(start.elapsed(), result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.temp.read_active_block_number().await
    }

    #[tracing::instrument(skip_all)]
    pub async fn read_mined_block_number(&self) -> anyhow::Result<BlockNumber> {
        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.perm.read_mined_block_number().await;
            metrics::inc_storage_read_mined_block_number(start.elapsed(), result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.perm.read_mined_block_number().await
    }

    #[tracing::instrument(skip_all)]
    pub async fn set_active_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.temp.set_active_block_number(number).await;
            metrics::inc_storage_set_active_block_number(start.elapsed(), result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.temp.set_active_block_number(number).await
    }

    pub async fn set_active_block_number_as_next(&self) -> anyhow::Result<()> {
        let last_mined_block = self.read_mined_block_number().await?;
        self.set_active_block_number(last_mined_block.next()).await?;
        Ok(())
    }

    pub async fn set_active_block_number_as_next_if_not_set(&self) -> anyhow::Result<()> {
        let active_block = self.read_active_block_number().await?;
        if active_block.is_none() {
            self.set_active_block_number_as_next().await?;
        }
        Ok(())
    }

    #[tracing::instrument(skip_all)]
    pub async fn set_mined_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.perm.set_mined_block_number(number).await;
            metrics::inc_storage_set_mined_block_number(start.elapsed(), result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.perm.set_mined_block_number(number).await
    }

    // -------------------------------------------------------------------------
    // Accounts and slots
    // -------------------------------------------------------------------------

    pub async fn set_active_external_block(&self, block: ExternalBlock) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.temp.set_active_external_block(block).await;
            metrics::inc_storage_set_active_external_block(start.elapsed(), result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.temp.set_active_external_block(block).await
    }

    #[tracing::instrument(skip_all)]
    pub async fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        // keep only accounts that does not exist in permanent storage
        let mut missing_accounts = Vec::new();
        for account in accounts {
            let perm_account = self.perm.read_account(&account.address, &StoragePointInTime::Present).await?;
            if perm_account.is_none() {
                missing_accounts.push(account);
            }
        }

        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.perm.save_accounts(missing_accounts).await;
            metrics::inc_storage_save_accounts(start.elapsed(), result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.perm.save_accounts(missing_accounts).await
    }

    #[tracing::instrument(skip_all)]
    pub async fn check_conflicts(&self, execution: &EvmExecution) -> anyhow::Result<Option<ExecutionConflicts>> {
        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.temp.check_conflicts(execution).await;
            metrics::inc_storage_check_conflicts(start.elapsed(), result.is_ok(), result.as_ref().is_ok_and(|conflicts| conflicts.is_some()));
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.temp.check_conflicts(execution).await
    }

    #[tracing::instrument(name = "storage::read_account", skip_all, fields(address, point_in_time))]
    pub async fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Account> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        // fill span
        Span::with(|s| {
            s.rec("address", address);
            s.rec("point_in_time", point_in_time);
        });

        // read from temp only if present
        if point_in_time.is_present() {
            if let Some(account) = self.temp.read_account(address).await? {
                tracing::debug!(%address, "account found in temporary storage");
                #[cfg(feature = "metrics")]
                metrics::inc_storage_read_account(start.elapsed(), label::TEMP, point_in_time, true);
                return Ok(account);
            }
        }

        // always read from perm if necessary
        match self.perm.read_account(address, point_in_time).await? {
            Some(account) => {
                tracing::debug!(%address, "account found in permanent storage");
                #[cfg(feature = "metrics")]
                metrics::inc_storage_read_account(start.elapsed(), label::PERM, point_in_time, true);
                Ok(account)
            }
            None => {
                tracing::debug!(%address, "account not found, assuming default value");
                #[cfg(feature = "metrics")]
                metrics::inc_storage_read_account(start.elapsed(), label::DEFAULT, point_in_time, true);
                Ok(Account::new_empty(*address))
            }
        }
    }

    #[tracing::instrument(skip_all)]
    pub async fn read_slot(&self, address: &Address, index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Slot> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        // read from temp only if present
        if point_in_time.is_present() {
            if let Some(slot) = self.temp.read_slot(address, index).await? {
                tracing::debug!(%address, %index, value = %slot.value, "slot found in temporary storage");
                #[cfg(feature = "metrics")]
                metrics::inc_storage_read_slot(start.elapsed(), label::TEMP, point_in_time, true);
                return Ok(slot);
            }
        }

        // always read from perm if necessary
        match self.perm.read_slot(address, index, point_in_time).await? {
            Some(slot) => {
                tracing::debug!(%address, %index, value = %slot.value, "slot found in permanent storage");
                #[cfg(feature = "metrics")]
                metrics::inc_storage_read_slot(start.elapsed(), label::PERM, point_in_time, true);
                Ok(slot)
            }
            None => {
                tracing::debug!(%address, %index, "slot not found, assuming default value");
                #[cfg(feature = "metrics")]
                metrics::inc_storage_read_slot(start.elapsed(), label::DEFAULT, point_in_time, true);
                Ok(Slot::new_empty(*index))
            }
        }
    }

    #[tracing::instrument(skip_all)]
    pub async fn read_slots(&self, address: &Address, slot_indexes: &SlotIndexes, point_in_time: &StoragePointInTime) -> anyhow::Result<Vec<Slot>> {
        #[cfg(feature = "metrics")]
        let start = metrics::now();

        let mut slots = Vec::with_capacity(slot_indexes.len());
        let mut perm_indexes = SlotIndexes::with_capacity(slot_indexes.len());

        // read from temp only if present
        if point_in_time.is_present() {
            for index in slot_indexes.iter() {
                match self.temp.read_slot(address, index).await? {
                    Some(slot) => {
                        slots.push(slot);
                    }
                    None => {
                        perm_indexes.insert(*index);
                    }
                }
            }
        }

        // read missing slots from permanent storage
        if not(perm_indexes.is_empty()) {
            let mut perm_slots = self.perm.read_slots(address, &perm_indexes, point_in_time).await?;
            for index in perm_indexes.0.into_iter() {
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

    #[tracing::instrument(skip_all)]
    pub async fn read_all_slots(&self, address: &Address) -> anyhow::Result<Vec<Slot>> {
        self.perm.read_all_slots(address).await
    }

    // -------------------------------------------------------------------------
    // Blocks
    // -------------------------------------------------------------------------

    #[tracing::instrument(skip_all)]
    pub async fn save_execution(&self, transaction_execution: TransactionExecution) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.temp.save_execution(transaction_execution).await;
            metrics::inc_storage_save_execution(start.elapsed(), result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.temp.save_execution(transaction_execution).await
    }

    #[tracing::instrument(skip_all)]
    pub async fn finish_block(&self) -> anyhow::Result<PendingBlock> {
        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.temp.finish_block().await;
            metrics::inc_storage_finish_block(start.elapsed(), result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.temp.finish_block().await
    }

    #[tracing::instrument(skip_all)]
    pub async fn save_block(&self, block: Block) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        {
            let (start, label_size_by_tx, label_size_by_gas) = (metrics::now(), block.label_size_by_transactions(), block.label_size_by_gas());
            let result = self.perm.save_block(block).await;
            metrics::inc_storage_save_block(start.elapsed(), label_size_by_tx, label_size_by_gas, result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.perm.save_block(block).await
    }

    #[tracing::instrument(skip_all)]
    pub async fn read_block(&self, block_selection: &BlockSelection) -> anyhow::Result<Option<Block>> {
        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.perm.read_block(block_selection).await;
            metrics::inc_storage_read_block(start.elapsed(), result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.perm.read_block(block_selection).await
    }

    #[tracing::instrument(skip_all)]
    pub async fn read_mined_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.perm.read_mined_transaction(hash).await;
            metrics::inc_storage_read_mined_transaction(start.elapsed(), result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.perm.read_mined_transaction(hash).await
    }

    #[tracing::instrument(skip_all)]
    pub async fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.perm.read_logs(filter).await;
            metrics::inc_storage_read_logs(start.elapsed(), result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.perm.read_logs(filter).await
    }

    #[tracing::instrument(skip_all)]
    pub async fn flush(&self) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.temp.flush().await;
            metrics::inc_storage_flush(start.elapsed(), label::TEMP, result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        {
            self.temp.flush().await?;
            Ok(())
        }
    }

    // -------------------------------------------------------------------------
    // General state
    // -------------------------------------------------------------------------

    #[tracing::instrument(skip_all)]
    pub async fn reset(&self, number: BlockNumber) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.perm.reset_at(number).await;
            metrics::inc_storage_reset(start.elapsed(), label::PERM, result.is_ok());

            let start = metrics::now();
            let result = self.temp.reset().await;
            metrics::inc_storage_reset(start.elapsed(), label::TEMP, result.is_ok());

            self.set_active_block_number_as_next().await?;

            Ok(())
        }

        #[cfg(not(feature = "metrics"))]
        {
            self.perm.reset_at(number).await?;
            self.temp.reset().await?;
            self.set_active_block_number_as_next().await?;
            Ok(())
        }
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
