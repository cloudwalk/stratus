use std::sync::Arc;

use clap::Parser;
use display_json::DebugAsJson;
use tracing::Span;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
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
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionStage;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::PermanentStorageConfig;
use crate::eth::storage::StoragePointInTime;
use crate::eth::storage::TemporaryStorage;
use crate::eth::storage::TemporaryStorageConfig;
use crate::ext::not;
use crate::infra::metrics;
use crate::infra::metrics::timed;
use crate::infra::tracing::SpanExt;

cfg_if::cfg_if! {
    if #[cfg(test)] {
        use crate::eth::storage::InMemoryPermanentStorage;
        use crate::eth::storage::InMemoryTemporaryStorage;
        use crate::eth::storage::RocksPermanentStorage;
    }
}

mod label {
    pub(super) const TEMP: &str = "temporary";
    pub(super) const PERM: &str = "permanent";
}

/// Proxy that simplifies interaction with permanent and temporary storages.
///
/// Additionaly it tracks metrics that are independent of the storage implementation.
pub struct StratusStorage {
    temp: Box<dyn TemporaryStorage>,
    perm: Box<dyn PermanentStorage>,
}

impl StratusStorage {
    // -------------------------------------------------------------------------
    // Initialization
    // -------------------------------------------------------------------------

    /// Creates a new storage with the specified temporary and permanent implementations.
    pub fn new(temp: Box<dyn TemporaryStorage>, perm: Box<dyn PermanentStorage>) -> Self {
        Self { temp, perm }
    }

    /// Creates an inmemory stratus storage for testing.
    #[cfg(test)]
    pub fn mock_new() -> Self {
        Self {
            temp: Box::new(InMemoryTemporaryStorage::new()),
            perm: Box::new(InMemoryPermanentStorage::new()),
        }
    }

    /// Creates an inmemory stratus storage for testing.
    #[cfg(test)]
    pub fn mock_new_rocksdb() -> (Self, tempfile::TempDir) {
        // Create a unique temporary directory within the ./data directory
        let temp_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
        let temp_path = temp_dir.path().to_str().expect("Failed to get temp path").to_string();

        let rocks_permanent_storage = RocksPermanentStorage::new(Some(temp_path.clone())).expect("Failed to create RocksPermanentStorage");

        (
            Self {
                temp: Box::new(InMemoryTemporaryStorage::new()),
                perm: Box::new(rocks_permanent_storage),
            },
            temp_dir,
        )
    }

    // -------------------------------------------------------------------------
    // Block number
    // -------------------------------------------------------------------------

    pub fn read_block_number_to_resume_import(&self) -> Result<BlockNumber, StratusError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_block_number_to_resume_import").entered();

        // if does not have the zero block present, should resume from zero
        let zero = self.read_block(&BlockFilter::Number(BlockNumber::ZERO))?;
        if zero.is_none() {
            tracing::info!(block_number = %0, reason = %"block ZERO does not exist", "resume from ZERO");
            return Ok(BlockNumber::ZERO);
        }

        // try to resume from pending block number
        let pending_block_number = self.read_pending_block_number()?;
        if let Some(number) = pending_block_number {
            tracing::info!(block_number = %number, reason = %"set in storage", "resume from PENDING");
            return Ok(number);
        }

        // fallback to last mined block number
        let mined_number = self.read_mined_block_number()?;
        let mined_block = self.read_block(&BlockFilter::Number(mined_number))?;
        match mined_block {
            Some(_) => {
                tracing::info!(block_number = %mined_number, reason = %"set in storage and block exist", "resume from MINED + 1");
                Ok(mined_number.next())
            }
            None => {
                tracing::info!(block_number = %mined_number, reason = %"set in storage but block does not exist", "resume from MINED");
                Ok(mined_number)
            }
        }
    }

    pub fn read_pending_block_number(&self) -> Result<Option<BlockNumber>, StratusError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_pending_block_number").entered();
        tracing::debug!(storage = %label::TEMP, "reading pending block number");

        timed(|| self.temp.read_pending_block_number())
            .with(|m| {
                metrics::inc_storage_read_pending_block_number(m.elapsed, label::TEMP, m.result.is_ok());
                if let Err(ref e) = m.result {
                    tracing::error!(reason = ?e, "failed to read pending block number");
                }
            })
            .map_err(Into::into)
    }

    pub fn read_mined_block_number(&self) -> Result<BlockNumber, StratusError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_mined_block_number").entered();
        tracing::debug!(storage = %label::PERM, "reading mined block number");

        timed(|| self.perm.read_mined_block_number())
            .with(|m| {
                metrics::inc_storage_read_mined_block_number(m.elapsed, label::PERM, m.result.is_ok());
                if let Err(ref e) = m.result {
                    tracing::error!(reason = ?e, "failed to read miner block number");
                }
            })
            .map_err(Into::into)
    }

    pub fn set_pending_block_number(&self, block_number: BlockNumber) -> Result<(), StratusError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::set_pending_block_number", %block_number).entered();
        tracing::debug!(storage = &label::TEMP, %block_number, "setting pending block number");

        timed(|| self.temp.set_pending_block_number(block_number))
            .with(|m| {
                metrics::inc_storage_set_pending_block_number(m.elapsed, label::TEMP, m.result.is_ok());
                if let Err(ref e) = m.result {
                    tracing::error!(reason = ?e, "failed to set pending block number");
                }
            })
            .map_err(Into::into)
    }

    pub fn set_pending_block_number_as_next(&self) -> Result<(), StratusError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::set_pending_block_number_as_next").entered();

        let last_mined_block = self.read_mined_block_number()?;
        self.set_pending_block_number(last_mined_block.next())?;
        Ok(())
    }

    pub fn set_pending_block_number_as_next_if_not_set(&self) -> Result<(), StratusError> {
        let pending_block = self.read_pending_block_number()?;
        if pending_block.is_none() {
            self.set_pending_block_number_as_next()?;
        }
        Ok(())
    }

    pub fn set_mined_block_number(&self, block_number: BlockNumber) -> Result<(), StratusError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::set_mined_block_number", %block_number).entered();
        tracing::debug!(storage = %label::PERM, %block_number, "setting mined block number");

        timed(|| self.perm.set_mined_block_number(block_number))
            .with(|m| {
                metrics::inc_storage_set_mined_block_number(m.elapsed, label::PERM, m.result.is_ok());
                if let Err(ref e) = m.result {
                    tracing::error!(reason = ?e, "failed to set miner block number");
                }
            })
            .map_err(Into::into)
    }

    // -------------------------------------------------------------------------
    // Accounts and slots
    // -------------------------------------------------------------------------

    pub fn set_pending_external_block(&self, block: ExternalBlock) -> Result<(), StratusError> {
        tracing::debug!(storage = %label::TEMP, block_number = %block.number(), "setting pending external block");

        timed(|| self.temp.set_pending_external_block(block))
            .with(|m| {
                metrics::inc_storage_set_pending_external_block(m.elapsed, label::TEMP, m.result.is_ok());
                if let Err(ref e) = m.result {
                    tracing::error!(reason = ?e, "failed to set pending external block");
                }
            })
            .map_err(Into::into)
    }

    pub fn save_accounts(&self, accounts: Vec<Account>) -> Result<(), StratusError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::save_accounts").entered();

        // keep only accounts that does not exist in permanent storage
        let mut missing_accounts = Vec::new();
        for account in accounts {
            let perm_account = self.perm.read_account(&account.address, &StoragePointInTime::Mined)?;
            if perm_account.is_none() {
                missing_accounts.push(account);
            }
        }

        tracing::debug!(storage = %label::PERM, accounts = ?missing_accounts, "saving initial accounts");
        timed(|| self.perm.save_accounts(missing_accounts))
            .with(|m| {
                metrics::inc_storage_save_accounts(m.elapsed, label::PERM, m.result.is_ok());
                if let Err(ref e) = m.result {
                    tracing::error!(reason = ?e, "failed to save accounts");
                }
            })
            .map_err(Into::into)
    }

    pub fn check_conflicts(&self, execution: &EvmExecution) -> Result<Option<ExecutionConflicts>, StratusError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("storage::check_conflicts").entered();
        tracing::debug!(storage = %label::TEMP, "checking conflicts");

        timed(|| self.temp.check_conflicts(execution))
            .with(|m| {
                metrics::inc_storage_check_conflicts(
                    m.elapsed,
                    label::TEMP,
                    m.result.is_ok(),
                    m.result.as_ref().is_ok_and(|conflicts| conflicts.is_some()),
                );
                if let Err(ref e) = m.result {
                    tracing::error!(reason = ?e, "failed to check conflicts");
                }
            })
            .map_err(Into::into)
    }

    pub fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> Result<Account, StratusError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("storage::read_account", %address, %point_in_time).entered();

        // read from temp only if requested
        if point_in_time.is_pending() {
            tracing::debug!(storage = %label::TEMP, %address, "reading account");
            let temp_account = timed(|| self.temp.read_account(address)).with(|m| {
                metrics::inc_storage_read_account(m.elapsed, label::TEMP, point_in_time, m.result.is_ok());
                if let Err(ref e) = m.result {
                    tracing::error!(reason = ?e, "failed to read account from temporary storage");
                }
            })?;
            if let Some(account) = temp_account {
                tracing::debug!(storage = %label::TEMP, %address, ?account, "account found in temporary storage");
                return Ok(account);
            }
        }

        // always read from perm if necessary
        tracing::debug!(storage = %label::PERM, %address, "reading account");
        let perm_account = timed(|| self.perm.read_account(address, point_in_time)).with(|m| {
            metrics::inc_storage_read_account(m.elapsed, label::PERM, point_in_time, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to read account from permanent storage");
            }
        })?;
        match perm_account {
            Some(account) => {
                tracing::debug!(storage = %label::PERM, %address, ?account, "account found in permanent storage");
                Ok(account)
            }
            None => {
                tracing::debug!(storage = %label::PERM, %address, "account not found, assuming default value");
                Ok(Account::new_empty(*address))
            }
        }
    }

    pub fn read_slot(&self, address: &Address, index: &SlotIndex, point_in_time: &StoragePointInTime) -> Result<Slot, StratusError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("storage::read_slot", %address, %index, %point_in_time).entered();

        // read from temp only if requested
        if point_in_time.is_pending() {
            tracing::debug!(storage = %label::TEMP, %address, %index, "reading slot");
            let temp_slot = timed(|| self.temp.read_slot(address, index)).with(|m| {
                metrics::inc_storage_read_slot(m.elapsed, label::TEMP, point_in_time, m.result.is_ok());
                if let Err(ref e) = m.result {
                    tracing::error!(reason = ?e, "failed to read slot from temporary storage");
                }
            })?;
            if let Some(slot) = temp_slot {
                tracing::debug!(storage = %label::TEMP, %address, %index, value = %slot.value, "slot found in temporary storage");
                return Ok(slot);
            }
        }

        // always read from perm if necessary
        tracing::debug!(storage = %label::PERM, %address, %index, %point_in_time, "reading slot");
        let perm_slot = timed(|| self.perm.read_slot(address, index, point_in_time)).with(|m| {
            metrics::inc_storage_read_slot(m.elapsed, label::PERM, point_in_time, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to read slot from permanent storage");
            }
        })?;
        match perm_slot {
            Some(slot) => {
                tracing::debug!(storage = %label::PERM, %address, %index, value = %slot.value, "slot found in permanent storage");
                Ok(slot)
            }
            None => {
                tracing::debug!(storage = %label::PERM, %address, %index, "slot not found, assuming default value");
                Ok(Slot::new_empty(*index))
            }
        }
    }

    pub fn read_all_slots(&self, address: &Address, point_in_time: &StoragePointInTime) -> Result<Vec<Slot>, StratusError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_all_slots").entered();
        tracing::debug!(storage = %label::TEMP, "checking conflicts");

        tracing::info!(storage = %label::PERM, %address, %point_in_time, "reading all slots");
        self.perm.read_all_slots(address, point_in_time).map_err(Into::into)
    }

    // -------------------------------------------------------------------------
    // Blocks
    // -------------------------------------------------------------------------

    pub fn save_execution(&self, tx: TransactionExecution) -> Result<(), StratusError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::save_execution", tx_hash = %tx.hash()).entered();
        tracing::debug!(storage = %label::TEMP, tx_hash = %tx.hash(), "saving execution");

        timed(|| self.temp.save_execution(tx))
            .with(|m| {
                metrics::inc_storage_save_execution(m.elapsed, label::TEMP, m.result.is_ok());
                if let Err(ref e) = m.result {
                    tracing::error!(reason = ?e, "failed to save execution");
                }
            })
            .map_err(Into::into)
    }

    /// Retrieves pending transactions being mined.
    pub fn pending_transactions(&self) -> Result<Vec<TransactionExecution>, StratusError> {
        self.temp.pending_transactions().map_err(Into::into)
    }

    pub fn finish_pending_block(&self) -> Result<PendingBlock, StratusError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::finish_pending_block", block_number = tracing::field::Empty).entered();
        tracing::debug!(storage = %label::TEMP, "finishing pending block");

        let result = timed(|| self.temp.finish_pending_block())
            .with(|m| {
                metrics::inc_storage_finish_pending_block(m.elapsed, label::TEMP, m.result.is_ok());
                if let Err(ref e) = m.result {
                    tracing::error!(reason = ?e, "failed to finish pending block");
                }
            })
            .map_err(Into::into);

        if let Ok(ref block) = result {
            Span::with(|s| s.rec_str("block_number", &block.number));
        }

        result
    }

    pub fn save_block(&self, block: Block) -> Result<(), StratusError> {
        let block_number = block.number();

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::save_block", block_number = %block.number()).entered();
        tracing::debug!(storage = %label::PERM, block_number = %block_number, transactions_len = %block.transactions.len(), "saving block");

        // check mined number
        let mined_number = self.read_mined_block_number()?;
        if not(block_number.is_zero()) && block_number != mined_number.next() {
            tracing::error!(%block_number, %mined_number, "failed to save block because mismatch with mined block number");
            return Err(StratusError::MinedNumberConflict {
                new: block_number,
                mined: mined_number,
            });
        }

        // check pending number
        if let Some(pending_number) = self.read_pending_block_number()? {
            if block_number >= pending_number {
                tracing::error!(%block_number, %pending_number, "failed to save block because mismatch with pending block number");
                return Err(StratusError::PendingNumberConflict {
                    new: block_number,
                    pending: pending_number,
                });
            }
        }

        // check mined block
        let existing_block = self.read_block(&BlockFilter::Number(block_number))?;
        if existing_block.is_some() {
            tracing::error!(%block_number, %mined_number, "failed to save block because block with the same number already exists in the permanent storage");
            return Err(StratusError::BlockConflict { number: block_number });
        }

        // save block
        let (label_size_by_tx, label_size_by_gas) = (block.label_size_by_transactions(), block.label_size_by_gas());
        timed(|| self.perm.save_block(block))
            .with(|m| {
                metrics::inc_storage_save_block(m.elapsed, label::PERM, label_size_by_tx, label_size_by_gas, m.result.is_ok());
                if let Err(ref e) = m.result {
                    tracing::error!(reason = ?e, %block_number, "failed to save block");
                }
            })
            .map_err(Into::into)
    }

    pub fn read_block(&self, filter: &BlockFilter) -> Result<Option<Block>, StratusError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_block", %filter).entered();
        tracing::debug!(storage = %label::PERM, ?filter, "reading block");

        timed(|| self.perm.read_block(filter))
            .with(|m| {
                metrics::inc_storage_read_block(m.elapsed, label::PERM, m.result.is_ok());
                if let Err(ref e) = m.result {
                    tracing::error!(reason = ?e, "failed to read block");
                }
            })
            .map_err(Into::into)
    }

    pub fn read_transaction(&self, tx_hash: &Hash) -> Result<Option<TransactionStage>, StratusError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_transaction", %tx_hash).entered();

        // read from temp
        tracing::debug!(storage = %label::TEMP, %tx_hash, "reading transaction");
        let temp_tx = timed(|| self.temp.read_transaction(tx_hash)).with(|m| {
            metrics::inc_storage_read_transaction(m.elapsed, label::TEMP, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to read transaction from temporary storage");
            }
        })?;
        if let Some(tx_temp) = temp_tx {
            return Ok(Some(TransactionStage::new_executed(tx_temp)));
        }

        // read from perm
        tracing::debug!(storage = %label::PERM, %tx_hash, "reading transaction");
        let perm_tx = timed(|| self.perm.read_transaction(tx_hash)).with(|m| {
            metrics::inc_storage_read_transaction(m.elapsed, label::PERM, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to read transaction from permanent storage");
            }
        })?;
        match perm_tx {
            Some(tx) => Ok(Some(TransactionStage::new_mined(tx))),
            None => Ok(None),
        }
    }

    pub fn read_logs(&self, filter: &LogFilter) -> Result<Vec<LogMined>, StratusError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_logs", ?filter).entered();
        tracing::debug!(storage = %label::PERM, ?filter, "reading logs");

        timed(|| self.perm.read_logs(filter))
            .with(|m| {
                metrics::inc_storage_read_logs(m.elapsed, label::PERM, m.result.is_ok());
                if let Err(ref e) = m.result {
                    tracing::error!(reason = ?e, "failed to read logs");
                }
            })
            .map_err(Into::into)
    }

    // -------------------------------------------------------------------------
    // General state
    // -------------------------------------------------------------------------

    pub fn reset(&self, number: BlockNumber) -> Result<(), StratusError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::reset").entered();

        // reset perm
        tracing::debug!(storage = %label::PERM, "reseting storage");
        timed(|| self.perm.reset_at(number)).with(|m| {
            metrics::inc_storage_reset(m.elapsed, label::PERM, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to reset permanent storage");
            }
        })?;

        // reset temp
        tracing::debug!(storage = %label::TEMP, "reseting storage");
        timed(|| self.temp.reset()).with(|m| {
            metrics::inc_storage_reset(m.elapsed, label::TEMP, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to reset temporary storage");
            }
        })?;

        self.set_pending_block_number_as_next()?;

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Utils
    // -------------------------------------------------------------------------

    /// Translates a block filter to a specific storage point-in-time indicator.
    pub fn translate_to_point_in_time(&self, block_filter: &BlockFilter) -> Result<StoragePointInTime, StratusError> {
        match block_filter {
            BlockFilter::Pending => Ok(StoragePointInTime::Pending),
            BlockFilter::Latest => Ok(StoragePointInTime::Mined),
            BlockFilter::Earliest => Ok(StoragePointInTime::MinedPast(BlockNumber::ZERO)),
            BlockFilter::Number(number) => Ok(StoragePointInTime::MinedPast(*number)),
            BlockFilter::Hash(_) => match self.read_block(block_filter)? {
                Some(block) => Ok(StoragePointInTime::MinedPast(block.header.number)),
                None => Err(StratusError::BlockFilterInvalid { filter: *block_filter }),
            },
        }
    }

    pub fn read_slots_sample(&self, start: BlockNumber, end: BlockNumber, max_samples: u64, seed: u64) -> Result<Vec<SlotSample>, StratusError> {
        self.perm.read_slots_sample(start, end, max_samples, seed).map_err(Into::into)
    }
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
        let storage = StratusStorage::new(temp_storage, perm_storage);

        Ok(Arc::new(storage))
    }
}
