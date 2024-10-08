use std::sync::Arc;

use clap::Parser;
use display_json::DebugAsJson;
use tracing::Span;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::PendingBlock;
use crate::eth::primitives::PendingBlockHeader;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
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
    pub fn new(temp: Box<dyn TemporaryStorage>, perm: Box<dyn PermanentStorage>) -> Result<Self, StratusError> {
        let this = Self { temp, perm };

        // create genesis block and accounts if necessary
        #[cfg(feature = "dev")]
        {
            let genesis = this.read_block(&crate::eth::primitives::BlockFilter::Number(crate::eth::primitives::BlockNumber::ZERO))?;
            if genesis.is_none() {
                this.reset_to_genesis()?;
            }
        }

        this.set_pending_block_number_as_next_if_not_set()?;

        Ok(this)
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
        let pending_header = self.read_pending_block_header()?;
        if let Some(pending_header) = pending_header {
            tracing::info!(block_number = %pending_header.number, reason = %"set in storage", "resume from PENDING");
            return Ok(pending_header.number);
        }

        // fallback to last mined block number
        let mined_number = self.read_mined_block_number()?;
        let mined_block = self.read_block(&BlockFilter::Number(mined_number))?;
        match mined_block {
            Some(_) => {
                tracing::info!(block_number = %mined_number, reason = %"set in storage and block exist", "resume from MINED + 1");
                Ok(mined_number.next_block_number())
            }
            None => {
                tracing::info!(block_number = %mined_number, reason = %"set in storage but block does not exist", "resume from MINED");
                Ok(mined_number)
            }
        }
    }

    pub fn read_pending_block_header(&self) -> Result<Option<PendingBlockHeader>, StratusError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_pending_block_number").entered();
        tracing::debug!(storage = %label::TEMP, "reading pending block number");

        timed(|| self.temp.read_pending_block_header())
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
        self.set_pending_block_number(last_mined_block.next_block_number())?;
        Ok(())
    }

    pub fn set_pending_block_number_as_next_if_not_set(&self) -> Result<(), StratusError> {
        let pending_block = self.read_pending_block_header()?;
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

    // -------------------------------------------------------------------------
    // Blocks
    // -------------------------------------------------------------------------

    pub fn save_execution(&self, tx: TransactionExecution, check_conflicts: bool) -> Result<(), StratusError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::save_execution", tx_hash = %tx.hash()).entered();
        tracing::debug!(storage = %label::TEMP, tx_hash = %tx.hash(), "saving execution");

        timed(|| self.temp.save_pending_execution(tx, check_conflicts))
            .with(|m| {
                metrics::inc_storage_save_execution(m.elapsed, label::TEMP, m.result.is_ok());
                if let Err(ref e) = m.result {
                    tracing::error!(reason = ?e, "failed to save execution");
                }
            })
            .map_err(Into::into)
    }

    /// Retrieves pending transactions being mined.
    pub fn pending_transactions(&self) -> Vec<TransactionExecution> {
        self.temp.read_pending_executions()
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
            Span::with(|s| s.rec_str("block_number", &block.header.number));
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
        if not(block_number.is_zero()) && block_number != mined_number.next_block_number() {
            tracing::error!(%block_number, %mined_number, "failed to save block because mismatch with mined block number");
            return Err(StratusError::StorageMinedNumberConflict {
                new: block_number,
                mined: mined_number,
            });
        }

        // check pending number
        if let Some(pending_header) = self.read_pending_block_header()? {
            if block_number >= pending_header.number {
                tracing::error!(%block_number, pending_number = %pending_header.number, "failed to save block because mismatch with pending block number");
                return Err(StratusError::StoragePendingNumberConflict {
                    new: block_number,
                    pending: pending_header.number,
                });
            }
        }

        // check mined block
        let existing_block = self.read_block(&BlockFilter::Number(block_number))?;
        if existing_block.is_some() {
            tracing::error!(%block_number, %mined_number, "failed to save block because block with the same number already exists in the permanent storage");
            return Err(StratusError::StorageBlockConflict { number: block_number });
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
        let temp_tx = timed(|| self.temp.read_pending_execution(tx_hash)).with(|m| {
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

    #[cfg(feature = "dev")]
    /// Resets the storage to the genesis state used in dev-mode.
    ///
    /// TODO: For now it uses the dev genesis block and test accounts, but it should be refactored to support genesis.json files.
    pub fn reset_to_genesis(&self) -> Result<(), StratusError> {
        use crate::eth::primitives::test_accounts;

        tracing::info!("reseting storage to genesis state");

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::reset").entered();

        // reset perm
        tracing::debug!(storage = %label::PERM, "reseting permanent storage");
        timed(|| self.perm.reset()).with(|m| {
            metrics::inc_storage_reset(m.elapsed, label::PERM, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to reset permanent storage");
            }
        })?;

        // reset temp
        tracing::debug!(storage = %label::TEMP, "reseting temporary storage");
        timed(|| self.temp.reset()).with(|m| {
            metrics::inc_storage_reset(m.elapsed, label::TEMP, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to reset temporary storage");
            }
        })?;

        // genesis block
        self.save_block(Block::genesis())?;

        // test accounts
        self.save_accounts(test_accounts())?;

        // block number
        self.set_mined_block_number(BlockNumber::ZERO)?;
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
                None => Err(StratusError::RpcBlockFilterInvalid { filter: *block_filter }),
            },
        }
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
        let storage = StratusStorage::new(temp_storage, perm_storage)?;

        Ok(Arc::new(storage))
    }
}
