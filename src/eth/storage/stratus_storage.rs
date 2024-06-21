use std::sync::Arc;

use anyhow::anyhow;
use tracing::Span;

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
use crate::eth::primitives::SlotSample;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::TemporaryStorage;
use crate::ext::SpanExt;

cfg_if::cfg_if! {
    if #[cfg(test)] {
        use crate::eth::storage::InMemoryPermanentStorage;
        use crate::eth::storage::InMemoryTemporaryStorage;
    }
}

cfg_if::cfg_if! {
    if #[cfg(feature = "metrics")] {
        use crate::infra::metrics;

        mod label {
            pub(super) const TEMP: &str = "temporary";
            pub(super) const PERM: &str = "permanent";
            pub(super) const DEFAULT: &str = "default";
        }
    }
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

    /// Creates an inmemory stratus storage for testing.
    #[cfg(test)]
    pub fn mock_new() -> Self {
        Self {
            temp: Arc::new(InMemoryTemporaryStorage::new()),
            perm: Arc::new(InMemoryPermanentStorage::new()),
        }
    }

    /// Creates an inmemory stratus storage for testing.
    #[cfg(test)]
    pub fn mock_new_rocksdb() -> (Self, tempfile::TempDir) {
        // Create a unique temporary directory within the ./data directory
        let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
        let temp_path = temp_dir.path().to_str().expect("Failed to get temp path").to_string();

        let rocks_permanent_storage =
            crate::eth::storage::RocksPermanentStorage::new(false, Some(temp_path.clone())).expect("Failed to create RocksPermanentStorage");

        (
            Self {
                temp: Arc::new(InMemoryTemporaryStorage::new()),
                perm: Arc::new(rocks_permanent_storage),
            },
            temp_dir,
        )
    }

    // -------------------------------------------------------------------------
    // Block number
    // -------------------------------------------------------------------------

    #[tracing::instrument(name = "storage::read_block_number_to_resume_import", skip_all)]
    pub fn read_block_number_to_resume_import(&self) -> anyhow::Result<BlockNumber> {
        // if does not have the zero block present, should resume from zero
        let zero = self.read_block(&BlockSelection::Number(BlockNumber::ZERO))?;
        if zero.is_none() {
            tracing::info!(number = %0, reason = %"block ZERO does not exist", "resume from ZERO");
            return Ok(BlockNumber::ZERO);
        }

        // try to resume from active block number
        let active_number = self.read_active_block_number()?;
        if let Some(active_number) = active_number {
            tracing::info!(number = %active_number, reason = %"set in storage", "resume from ACTIVE");
            return Ok(active_number);
        }

        // fallback to last mined block number
        let mined_number = self.read_mined_block_number()?;
        let mined_block = self.read_block(&BlockSelection::Number(mined_number))?;
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

    #[tracing::instrument(name = "storage::read_active_block_number", skip_all)]
    pub fn read_active_block_number(&self) -> anyhow::Result<Option<BlockNumber>> {
        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.temp.read_active_block_number();
            metrics::inc_storage_read_active_block_number(start.elapsed(), result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.temp.read_active_block_number()
    }

    #[tracing::instrument(name = "storage::read_mined_block_number", skip_all)]
    pub fn read_mined_block_number(&self) -> anyhow::Result<BlockNumber> {
        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.perm.read_mined_block_number();
            metrics::inc_storage_read_mined_block_number(start.elapsed(), result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.perm.read_mined_block_number()
    }

    #[tracing::instrument(name = "storage::set_active_block_number", skip_all, fields(number))]
    pub fn set_active_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        Span::with(|s| {
            s.rec_str("number", &number);
        });

        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.temp.set_active_block_number(number);
            metrics::inc_storage_set_active_block_number(start.elapsed(), result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.temp.set_active_block_number(number)
    }

    #[tracing::instrument(name = "storage::set_active_block_number_as_next", skip_all)]
    pub fn set_active_block_number_as_next(&self) -> anyhow::Result<()> {
        let last_mined_block = self.read_mined_block_number()?;
        self.set_active_block_number(last_mined_block.next())?;
        Ok(())
    }

    pub fn set_active_block_number_as_next_if_not_set(&self) -> anyhow::Result<()> {
        let active_block = self.read_active_block_number()?;
        if active_block.is_none() {
            self.set_active_block_number_as_next()?;
        }
        Ok(())
    }

    #[tracing::instrument(name = "storage::set_mined_block_number", skip_all, fields(number))]
    pub fn set_mined_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        Span::with(|s| s.rec_str("number", &number));

        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.perm.set_mined_block_number(number);
            metrics::inc_storage_set_mined_block_number(start.elapsed(), result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.perm.set_mined_block_number(number)
    }

    // -------------------------------------------------------------------------
    // Accounts and slots
    // -------------------------------------------------------------------------

    pub fn set_active_external_block(&self, block: ExternalBlock) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.temp.set_active_external_block(block);
            metrics::inc_storage_set_active_external_block(start.elapsed(), result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.temp.set_active_external_block(block)
    }

    #[tracing::instrument(name = "storage::save_accounts", skip_all)]
    pub fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        // keep only accounts that does not exist in permanent storage
        let mut missing_accounts = Vec::new();
        for account in accounts {
            let perm_account = self.perm.read_account(&account.address, &StoragePointInTime::Present)?;
            if perm_account.is_none() {
                missing_accounts.push(account);
            }
        }

        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.perm.save_accounts(missing_accounts);
            metrics::inc_storage_save_accounts(start.elapsed(), result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.perm.save_accounts(missing_accounts)
    }

    #[tracing::instrument(name = "storage::check_conflicts", skip_all)]
    pub fn check_conflicts(&self, execution: &EvmExecution) -> anyhow::Result<Option<ExecutionConflicts>> {
        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.temp.check_conflicts(execution);
            metrics::inc_storage_check_conflicts(start.elapsed(), result.is_ok(), result.as_ref().is_ok_and(|conflicts| conflicts.is_some()));
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.temp.check_conflicts(execution)
    }

    #[tracing::instrument(name = "storage::read_account", skip_all, fields(address, point_in_time))]
    pub fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Account> {
        Span::with(|s| {
            s.rec_str("address", address);
            s.rec_str("point_in_time", point_in_time);
        });

        #[cfg(feature = "metrics")]
        let start = metrics::now();

        // read from temp only if present
        if point_in_time.is_present() {
            if let Some(account) = self.temp.read_account(address)? {
                tracing::debug!(%address, "account found in temporary storage");
                #[cfg(feature = "metrics")]
                metrics::inc_storage_read_account(start.elapsed(), label::TEMP, point_in_time, true);
                return Ok(account);
            }
        }

        // always read from perm if necessary
        match self.perm.read_account(address, point_in_time)? {
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

    #[tracing::instrument(name = "storage::read_slot", skip_all, fields(address, index, point_in_time))]
    pub fn read_slot(&self, address: &Address, index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Slot> {
        Span::with(|s| {
            s.rec_str("address", address);
            s.rec_str("index", index);
            s.rec_str("point_in_time", point_in_time);
        });

        #[cfg(feature = "metrics")]
        let start = metrics::now();

        // read from temp only if present
        if point_in_time.is_present() {
            if let Some(slot) = self.temp.read_slot(address, index)? {
                tracing::debug!(%address, %index, value = %slot.value, "slot found in temporary storage");
                #[cfg(feature = "metrics")]
                metrics::inc_storage_read_slot(start.elapsed(), label::TEMP, point_in_time, true);
                return Ok(slot);
            }
        }

        // always read from perm if necessary
        match self.perm.read_slot(address, index, point_in_time)? {
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

    #[tracing::instrument(name = "storage::read_all_slots", skip_all)]
    pub fn read_all_slots(&self, address: &Address) -> anyhow::Result<Vec<Slot>> {
        self.perm.read_all_slots(address)
    }

    // -------------------------------------------------------------------------
    // Blocks
    // -------------------------------------------------------------------------

    #[tracing::instrument(name = "storage::save_execution", skip_all, fields(hash))]
    pub fn save_execution(&self, tx: TransactionExecution) -> anyhow::Result<()> {
        Span::with(|s| {
            s.rec_str("hash", &tx.hash());
        });

        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.temp.save_execution(tx);
            metrics::inc_storage_save_execution(start.elapsed(), result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.temp.save_execution(tx)
    }

    #[tracing::instrument(name = "storage::finish_block", skip_all, fields(number))]
    pub fn finish_block(&self) -> anyhow::Result<PendingBlock> {
        #[cfg(feature = "metrics")]
        let result = {
            let start = metrics::now();
            let result = self.temp.finish_block();
            metrics::inc_storage_finish_block(start.elapsed(), result.is_ok());
            result
        };

        #[cfg(not(feature = "metrics"))]
        let result = self.temp.finish_block();

        if let Ok(ref block) = result {
            Span::with(|s| s.rec_str("number", &block.number));
        }

        result
    }

    #[tracing::instrument(name = "storage::save_block", skip_all, fields(number))]
    pub fn save_block(&self, block: Block) -> anyhow::Result<()> {
        Span::with(|s| s.rec_str("number", &block.number()));

        #[cfg(feature = "metrics")]
        {
            let (start, label_size_by_tx, label_size_by_gas) = (metrics::now(), block.label_size_by_transactions(), block.label_size_by_gas());
            let result = self.perm.save_block(block);
            metrics::inc_storage_save_block(start.elapsed(), label_size_by_tx, label_size_by_gas, result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.perm.save_block(block)
    }

    #[tracing::instrument(name = "storage::read_block", skip_all)]
    pub fn read_block(&self, block_selection: &BlockSelection) -> anyhow::Result<Option<Block>> {
        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.perm.read_block(block_selection);
            metrics::inc_storage_read_block(start.elapsed(), result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.perm.read_block(block_selection)
    }

    #[tracing::instrument(name = "storage::read_transaction", skip_all, fields(hash))]
    pub fn read_mined_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        Span::with(|s| s.rec_str("hash", hash));

        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.perm.read_mined_transaction(hash);
            metrics::inc_storage_read_mined_transaction(start.elapsed(), result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.perm.read_mined_transaction(hash)
    }

    #[tracing::instrument(name = "storage::read_logs", skip_all)]
    pub fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.perm.read_logs(filter);
            metrics::inc_storage_read_logs(start.elapsed(), result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        self.perm.read_logs(filter)
    }

    // -------------------------------------------------------------------------
    // General state
    // -------------------------------------------------------------------------

    #[tracing::instrument(name = "storage::flush", skip_all)]
    pub fn flush(&self) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.temp.flush();
            metrics::inc_storage_flush(start.elapsed(), label::TEMP, result.is_ok());
            result
        }

        #[cfg(not(feature = "metrics"))]
        {
            self.temp.flush()?;
            Ok(())
        }
    }

    #[tracing::instrument(name = "storage::reset", skip_all)]
    pub fn reset(&self, number: BlockNumber) -> anyhow::Result<()> {
        #[cfg(feature = "metrics")]
        {
            let start = metrics::now();
            let result = self.perm.reset_at(number);
            metrics::inc_storage_reset(start.elapsed(), label::PERM, result.is_ok());

            let start = metrics::now();
            let result = self.temp.reset();
            metrics::inc_storage_reset(start.elapsed(), label::TEMP, result.is_ok());

            self.set_active_block_number_as_next()?;

            Ok(())
        }

        #[cfg(not(feature = "metrics"))]
        {
            self.perm.reset_at(number)?;
            self.temp.reset()?;
            self.set_active_block_number_as_next()?;
            Ok(())
        }
    }

    // -------------------------------------------------------------------------
    // Utils
    // -------------------------------------------------------------------------

    /// Translates a block selection to a specific storage point-in-time indicator.
    pub fn translate_to_point_in_time(&self, block_selection: &BlockSelection) -> anyhow::Result<StoragePointInTime> {
        match block_selection {
            BlockSelection::Latest => Ok(StoragePointInTime::Present),
            BlockSelection::Number(number) => {
                let current_block = self.perm.read_mined_block_number()?;
                if number <= &current_block {
                    Ok(StoragePointInTime::Past(*number))
                } else {
                    Ok(StoragePointInTime::Past(current_block))
                }
            }
            BlockSelection::Earliest | BlockSelection::Hash(_) => match self.read_block(block_selection)? {
                Some(block) => Ok(StoragePointInTime::Past(block.header.number)),
                None => Err(anyhow!(
                    "failed to select block because it is greater than current block number or block hash is invalid"
                )),
            },
        }
    }

    pub fn read_slots_sample(&self, start: BlockNumber, end: BlockNumber, max_samples: u64, seed: u64) -> anyhow::Result<Vec<SlotSample>> {
        self.perm.read_slots_sample(start, end, max_samples, seed)
    }
}
