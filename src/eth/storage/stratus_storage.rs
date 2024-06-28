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
use crate::eth::primitives::TransactionStage;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::TemporaryStorage;
use crate::infra::metrics;
use crate::infra::metrics::timed;
use crate::infra::tracing::SpanExt;

cfg_if::cfg_if! {
    if #[cfg(test)] {
        use crate::eth::storage::InMemoryPermanentStorage;
        use crate::eth::storage::InMemoryTemporaryStorage;
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

        let rocks_permanent_storage = crate::eth::storage::RocksPermanentStorage::new(Some(temp_path.clone())).expect("Failed to create RocksPermanentStorage");

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

    #[tracing::instrument(name = "storage::read_block_number_to_resume_import", skip_all)]
    pub fn read_block_number_to_resume_import(&self) -> anyhow::Result<BlockNumber> {
        // if does not have the zero block present, should resume from zero
        let zero = self.read_block(&BlockSelection::Number(BlockNumber::ZERO))?;
        if zero.is_none() {
            tracing::info!(block_number = %0, reason = %"block ZERO does not exist", "resume from ZERO");
            return Ok(BlockNumber::ZERO);
        }

        // try to resume from active block number
        let active_number = self.read_active_block_number()?;
        if let Some(active_number) = active_number {
            tracing::info!(block_number = %active_number, reason = %"set in storage", "resume from ACTIVE");
            return Ok(active_number);
        }

        // fallback to last mined block number
        let mined_number = self.read_mined_block_number()?;
        let mined_block = self.read_block(&BlockSelection::Number(mined_number))?;
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

    #[tracing::instrument(name = "storage::read_active_block_number", skip_all)]
    pub fn read_active_block_number(&self) -> anyhow::Result<Option<BlockNumber>> {
        tracing::debug!(storage = %label::TEMP, "reading active block number");

        timed(|| self.temp.read_active_block_number()).with(|m| {
            metrics::inc_storage_read_active_block_number(m.elapsed, label::TEMP, m.result.is_ok());
        })
    }

    #[tracing::instrument(name = "storage::read_mined_block_number", skip_all)]
    pub fn read_mined_block_number(&self) -> anyhow::Result<BlockNumber> {
        tracing::debug!(storage = %label::PERM, "reading mined block number");

        timed(|| self.perm.read_mined_block_number()).with(|m| {
            metrics::inc_storage_read_mined_block_number(m.elapsed, label::PERM, m.result.is_ok());
        })
    }

    #[tracing::instrument(name = "storage::set_active_block_number", skip_all, fields(block_number))]
    pub fn set_active_block_number(&self, block_number: BlockNumber) -> anyhow::Result<()> {
        Span::with(|s| {
            s.rec_str("block_number", &block_number);
        });
        tracing::debug!(storage = &label::TEMP, %block_number, "setting active block number");

        timed(|| self.temp.set_active_block_number(block_number)).with(|m| {
            metrics::inc_storage_set_active_block_number(m.elapsed, label::TEMP, m.result.is_ok());
        })
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

    #[tracing::instrument(name = "storage::set_mined_block_number", skip_all, fields(block_number))]
    pub fn set_mined_block_number(&self, block_number: BlockNumber) -> anyhow::Result<()> {
        Span::with(|s| s.rec_str("block_number", &block_number));
        tracing::debug!(storage = %label::PERM, %block_number, "setting mined block number");

        timed(|| self.perm.set_mined_block_number(block_number)).with(|m| {
            metrics::inc_storage_set_mined_block_number(m.elapsed, label::PERM, m.result.is_ok());
        })
    }

    // -------------------------------------------------------------------------
    // Accounts and slots
    // -------------------------------------------------------------------------

    pub fn set_active_external_block(&self, block: ExternalBlock) -> anyhow::Result<()> {
        tracing::debug!(storage = %label::TEMP, block_number = %block.number(), "setting active external block");

        timed(|| self.temp.set_active_external_block(block)).with(|m| {
            metrics::inc_storage_set_active_external_block(m.elapsed, label::TEMP, m.result.is_ok());
        })
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

        tracing::debug!(storage = %label::PERM, accounts = ?missing_accounts, "saving initial accounts");
        timed(|| self.perm.save_accounts(missing_accounts)).with(|m| {
            metrics::inc_storage_save_accounts(m.elapsed, label::PERM, m.result.is_ok());
        })
    }

    #[tracing::instrument(name = "storage::check_conflicts", skip_all)]
    pub fn check_conflicts(&self, execution: &EvmExecution) -> anyhow::Result<Option<ExecutionConflicts>> {
        tracing::debug!(storage = %label::TEMP, "checking conflicts");

        timed(|| self.temp.check_conflicts(execution)).with(|m| {
            metrics::inc_storage_check_conflicts(
                m.elapsed,
                label::TEMP,
                m.result.is_ok(),
                m.result.as_ref().is_ok_and(|conflicts| conflicts.is_some()),
            );
        })
    }

    #[tracing::instrument(name = "storage::read_account", skip_all, fields(address, point_in_time))]
    pub fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Account> {
        Span::with(|s| {
            s.rec_str("address", address);
            s.rec_str("point_in_time", point_in_time);
        });

        // read from temp only if present
        if point_in_time.is_present() {
            tracing::debug!(storage = %label::TEMP, %address, "reading account");
            let temp_account = timed(|| self.temp.read_account(address)).with(|m| {
                metrics::inc_storage_read_account(m.elapsed, label::TEMP, point_in_time, m.result.is_ok());
            })?;
            if let Some(account) = temp_account {
                tracing::debug!(storage = %label::TEMP, %address, "account found in temporary storage");
                return Ok(account);
            }
        }

        // always read from perm if necessary
        tracing::debug!(storage = %label::PERM, %address, "reading account");
        let perm_account = timed(|| self.perm.read_account(address, point_in_time)).with(|m| {
            metrics::inc_storage_read_account(m.elapsed, label::PERM, point_in_time, m.result.is_ok());
        })?;
        match perm_account {
            Some(account) => {
                tracing::debug!(%address, "account found in permanent storage");
                Ok(account)
            }
            None => {
                tracing::debug!(%address, "account not found, assuming default value");
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

        // read from temp only if present
        if point_in_time.is_present() {
            tracing::debug!(storage = %label::TEMP, %address, %index, "reading slot");
            let temp_slot = timed(|| self.temp.read_slot(address, index)).with(|m| {
                metrics::inc_storage_read_slot(m.elapsed, label::TEMP, point_in_time, m.result.is_ok());
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
        })?;
        match perm_slot {
            Some(slot) => {
                tracing::debug!(%address, %index, value = %slot.value, "slot found in permanent storage");
                Ok(slot)
            }
            None => {
                tracing::debug!(%address, %index, "slot not found, assuming default value");
                Ok(Slot::new_empty(*index))
            }
        }
    }

    #[tracing::instrument(name = "storage::read_all_slots", skip_all)]
    pub fn read_all_slots(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Vec<Slot>> {
        tracing::info!(storage = %label::PERM, %address, %point_in_time, "reading all slots");
        self.perm.read_all_slots(address, point_in_time)
    }

    // -------------------------------------------------------------------------
    // Blocks
    // -------------------------------------------------------------------------

    #[tracing::instrument(name = "storage::save_execution", skip_all, fields(tx_hash))]
    pub fn save_execution(&self, tx: TransactionExecution) -> anyhow::Result<()> {
        Span::with(|s| {
            s.rec_str("tx_hash", &tx.hash());
        });
        tracing::debug!(storage = %label::TEMP, tx_hash = %tx.hash(), "saving execution");

        timed(|| self.temp.save_execution(tx)).with(|m| {
            metrics::inc_storage_save_execution(m.elapsed, label::TEMP, m.result.is_ok());
        })
    }

    #[tracing::instrument(name = "storage::finish_block", skip_all, fields(block_number))]
    pub fn finish_block(&self) -> anyhow::Result<PendingBlock> {
        tracing::debug!(storage = %label::TEMP, "finishing active block");

        let result = timed(|| self.temp.finish_block()).with(|m| {
            metrics::inc_storage_finish_block(m.elapsed, label::TEMP, m.result.is_ok());
        });

        if let Ok(ref block) = result {
            Span::with(|s| s.rec_str("block_number", &block.number));
        }

        result
    }

    #[tracing::instrument(name = "storage::save_block", skip_all, fields(block_number))]
    pub fn save_block(&self, block: Block) -> anyhow::Result<()> {
        Span::with(|s| s.rec_str("block_number", &block.number()));
        tracing::debug!(storage = %label::PERM, block_number = %block.number(), transactions_len = %block.transactions.len(), "saving block");

        let (label_size_by_tx, label_size_by_gas) = (block.label_size_by_transactions(), block.label_size_by_gas());
        timed(|| self.perm.save_block(block)).with(|m| {
            metrics::inc_storage_save_block(m.elapsed, label::PERM, label_size_by_tx, label_size_by_gas, m.result.is_ok());
        })
    }

    #[tracing::instrument(name = "storage::read_block", skip_all)]
    pub fn read_block(&self, selection: &BlockSelection) -> anyhow::Result<Option<Block>> {
        tracing::debug!(storage = %label::PERM, ?selection, "reading block");

        timed(|| self.perm.read_block(selection)).with(|m| {
            metrics::inc_storage_read_block(m.elapsed, label::PERM, m.result.is_ok());
        })
    }

    #[tracing::instrument(name = "storage::read_transaction", skip_all, fields(tx_hash))]
    pub fn read_transaction(&self, tx_hash: &Hash) -> anyhow::Result<Option<TransactionStage>> {
        Span::with(|s| s.rec_str("tx_hash", tx_hash));

        // read from temp
        tracing::debug!(storage = %label::TEMP, %tx_hash, "reading transaction");
        let temp_tx = timed(|| self.temp.read_transaction(tx_hash)).with(|m| {
            metrics::inc_storage_read_transaction(m.elapsed, label::TEMP, m.result.is_ok());
        })?;
        if let Some(tx_temp) = temp_tx {
            return Ok(Some(TransactionStage::new_executed(tx_temp)));
        }

        // read from perm
        tracing::debug!(storage = %label::PERM, %tx_hash, "reading transaction");
        let perm_tx = timed(|| self.perm.read_transaction(tx_hash)).with(|m| {
            metrics::inc_storage_read_transaction(m.elapsed, label::PERM, m.result.is_ok());
        })?;
        match perm_tx {
            Some(tx) => Ok(Some(TransactionStage::new_mined(tx))),
            None => Ok(None),
        }
    }

    #[tracing::instrument(name = "storage::read_logs", skip_all)]
    pub fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        tracing::debug!(storage = %label::PERM, ?filter, "reading logs");

        timed(|| self.perm.read_logs(filter)).with(|m| {
            metrics::inc_storage_read_logs(m.elapsed, label::PERM, m.result.is_ok());
        })
    }

    // -------------------------------------------------------------------------
    // General state
    // -------------------------------------------------------------------------

    #[tracing::instrument(name = "storage::reset", skip_all)]
    pub fn reset(&self, number: BlockNumber) -> anyhow::Result<()> {
        // reset perm
        tracing::debug!(storage = %label::PERM, "reseting storage");
        timed(|| self.perm.reset_at(number)).with(|m| {
            metrics::inc_storage_reset(m.elapsed, label::PERM, m.result.is_ok());
        })?;

        // reset temp
        tracing::debug!(storage = %label::TEMP, "reseting storage");
        timed(|| self.temp.reset()).with(|m| {
            metrics::inc_storage_reset(m.elapsed, label::TEMP, m.result.is_ok());
        })?;

        self.set_active_block_number_as_next()?;

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Utils
    // -------------------------------------------------------------------------

    /// Translates a block selection to a specific storage point-in-time indicator.
    pub fn translate_to_point_in_time(&self, block_selection: &BlockSelection) -> anyhow::Result<StoragePointInTime> {
        match block_selection {
            BlockSelection::Latest => Ok(StoragePointInTime::Present),
            BlockSelection::Number(number) => Ok(StoragePointInTime::Past(*number)),
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
