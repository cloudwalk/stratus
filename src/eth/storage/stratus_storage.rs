use anyhow::anyhow;
use tracing::Span;

use super::StorageCache;
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
use crate::eth::primitives::PointInTime;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StorageError;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionStage;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::TemporaryStorage;
use crate::ext::not;
use crate::infra::metrics;
use crate::infra::metrics::timed;
use crate::infra::tracing::SpanExt;

mod label {
    pub(super) const TEMP: &str = "temporary";
    pub(super) const PERM: &str = "permanent";
    pub(super) const CACHE: &str = "cache";
}

/// Proxy that simplifies interaction with permanent and temporary storages.
///
/// Additionaly it tracks metrics that are independent of the storage implementation.
pub struct StratusStorage {
    temp: Box<dyn TemporaryStorage>,
    cache: StorageCache,
    perm: Box<dyn PermanentStorage>,
}

impl StratusStorage {
    /// Creates a new storage with the specified temporary and permanent implementations.
    pub fn new(temp: Box<dyn TemporaryStorage>, perm: Box<dyn PermanentStorage>, cache: StorageCache) -> Result<Self, StorageError> {
        let this = Self { temp, cache, perm };

        // create genesis block and accounts if necessary
        #[cfg(feature = "dev")]
        {
            let genesis = this.read_block(BlockFilter::Number(BlockNumber::ZERO))?;
            if genesis.is_none() {
                this.reset_to_genesis()?;
            }
        }

        Ok(this)
    }

    #[cfg(test)]
    pub fn new_test() -> Result<Self, StorageError> {
        use super::cache::CacheConfig;

        let perm = Box::new(super::InMemoryPermanentStorage::default());
        let temp = Box::new(super::InMemoryTemporaryStorage::new(0.into()));
        let cache = CacheConfig {
            slot_cache_capacity: 100000,
            account_cache_capacity: 20000,
        }
        .init();

        Self::new(temp, perm, cache)
    }

    pub fn read_block_number_to_resume_import(&self) -> Result<BlockNumber, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_block_number_to_resume_import").entered();
        Ok(self.read_pending_block_header().number)
    }

    pub fn read_pending_block_header(&self) -> PendingBlockHeader {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_pending_block_number").entered();
        tracing::debug!(storage = %label::TEMP, "reading pending block number");

        timed(|| self.temp.read_pending_block_header()).with(|m| {
            metrics::inc_storage_read_pending_block_number(m.elapsed, label::TEMP, true);
        })
    }

    pub fn read_mined_block_number(&self) -> Result<BlockNumber, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_mined_block_number").entered();
        tracing::debug!(storage = %label::PERM, "reading mined block number");

        timed(|| self.perm.read_mined_block_number()).with(|m| {
            metrics::inc_storage_read_mined_block_number(m.elapsed, label::PERM, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to read miner block number");
            }
        })
    }

    pub fn set_mined_block_number(&self, block_number: BlockNumber) -> Result<(), StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::set_mined_block_number", %block_number).entered();
        tracing::debug!(storage = %label::PERM, %block_number, "setting mined block number");

        timed(|| self.perm.set_mined_block_number(block_number)).with(|m| {
            metrics::inc_storage_set_mined_block_number(m.elapsed, label::PERM, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to set miner block number");
            }
        })
    }

    // -------------------------------------------------------------------------
    // Accounts and slots
    // -------------------------------------------------------------------------

    pub fn save_accounts(&self, accounts: Vec<Account>) -> Result<(), StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::save_accounts").entered();

        // keep only accounts that does not exist in permanent storage
        let mut missing_accounts = Vec::new();
        for account in accounts {
            let perm_account = self.perm.read_account(account.address, PointInTime::Mined)?;
            if perm_account.is_none() {
                missing_accounts.push(account);
            }
        }

        tracing::debug!(storage = %label::PERM, accounts = ?missing_accounts, "saving initial accounts");
        timed(|| self.perm.save_accounts(missing_accounts)).with(|m| {
            metrics::inc_storage_save_accounts(m.elapsed, label::PERM, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to save accounts");
            }
        })
    }

    pub fn read_account(&self, address: Address, point_in_time: PointInTime) -> Result<Account, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("storage::read_account", %address, %point_in_time).entered();

        let account = 'query: {
            if point_in_time.is_pending() {
                if let Some(account) = timed(|| self.cache.get_account(address)).with(|m| {
                    metrics::inc_storage_read_account(m.elapsed, label::CACHE, point_in_time, true);
                }) {
                    tracing::debug!(storage = %label::CACHE, %address, ?account, "account found in cache");
                    return Ok(account);
                };

                tracing::debug!(storage = %label::TEMP, %address, "reading account");
                let temp_account = timed(|| self.temp.read_account(address)).with(|m| {
                    metrics::inc_storage_read_account(m.elapsed, label::TEMP, point_in_time, m.result.is_ok());
                    if let Err(ref e) = m.result {
                        tracing::error!(reason = ?e, "failed to read account from temporary storage");
                    }
                })?;
                if let Some(account) = temp_account {
                    tracing::debug!(storage = %label::TEMP, %address, ?account, "account found in temporary storage");
                    break 'query account;
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
                    account
                }
                None => {
                    tracing::debug!(storage = %label::PERM, %address, "account not found, assuming default value");
                    Account::new_empty(address)
                }
            }
        };

        if point_in_time.is_pending() {
            self.cache.cache_account(account.clone());
        }
        Ok(account)
    }

    pub fn read_slot(&self, address: Address, index: SlotIndex, point_in_time: PointInTime) -> Result<Slot, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("storage::read_slot", %address, %index, %point_in_time).entered();

        let slot = 'query: {
            if point_in_time.is_pending() {
                if let Some(slot) = timed(|| self.cache.get_slot(address, index)).with(|m| {
                    metrics::inc_storage_read_slot(m.elapsed, label::CACHE, point_in_time, true);
                }) {
                    tracing::debug!(storage = %label::CACHE, %address, %index, value = %slot.value, "slot found in cache");
                    return Ok(slot);
                };

                tracing::debug!(storage = %label::TEMP, %address, %index, "reading slot");
                let temp_slot = timed(|| self.temp.read_slot(address, index)).with(|m| {
                    metrics::inc_storage_read_slot(m.elapsed, label::TEMP, point_in_time, m.result.is_ok());
                    if let Err(ref e) = m.result {
                        tracing::error!(reason = ?e, "failed to read slot from temporary storage");
                    }
                })?;
                if let Some(slot) = temp_slot {
                    tracing::debug!(storage = %label::TEMP, %address, %index, value = %slot.value, "slot found in temporary storage");
                    break 'query slot;
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
                    slot
                }
                None => {
                    tracing::debug!(storage = %label::PERM, %address, %index, "slot not found, assuming default value");
                    Slot::new_empty(index)
                }
            }
        };

        if point_in_time.is_pending() {
            self.cache.cache_slot(address, slot);
        }
        Ok(slot)
    }

    // -------------------------------------------------------------------------
    // Blocks
    // -------------------------------------------------------------------------

    pub fn save_execution(&self, tx: TransactionExecution, check_conflicts: bool) -> Result<(), StorageError> {
        let changes = tx.execution().changes.clone();

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::save_execution", tx_hash = %tx.hash()).entered();
        tracing::debug!(storage = %label::TEMP, tx_hash = %tx.hash(), "saving execution");

        timed(|| self.temp.save_pending_execution(tx, check_conflicts))
            .with(|m| {
                metrics::inc_storage_save_execution(m.elapsed, label::TEMP, m.result.is_ok());
                match &m.result {
                    Err(StorageError::EvmInputMismatch { .. }) => {
                        tracing::warn!("failed to save execution due to mismatch, will retry");
                    }
                    Err(ref e) => tracing::error!(reason = ?e, "failed to save execution"),
                    _ => (),
                }
            })
            .inspect(|_| self.cache.cache_account_and_slots_from_changes(changes))
    }

    /// Retrieves pending transactions being mined.
    pub fn pending_transactions(&self) -> Vec<TransactionExecution> {
        self.temp.read_pending_executions()
    }

    pub fn finish_pending_block(&self) -> Result<PendingBlock, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::finish_pending_block", block_number = tracing::field::Empty).entered();
        tracing::debug!(storage = %label::TEMP, "finishing pending block");

        let result = timed(|| self.temp.finish_pending_block()).with(|m| {
            metrics::inc_storage_finish_pending_block(m.elapsed, label::TEMP, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to finish pending block");
            }
        });

        if let Ok(ref block) = result {
            Span::with(|s| s.rec_str("block_number", &block.header.number));
        }

        result
    }

    pub fn save_block(&self, block: Block) -> Result<(), StorageError> {
        let block_number = block.number();

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::save_block", block_number = %block.number()).entered();
        tracing::debug!(storage = %label::PERM, block_number = %block_number, transactions_len = %block.transactions.len(), "saving block");

        // check mined number
        let mined_number = self.read_mined_block_number()?;
        if not(block_number.is_zero()) && block_number != mined_number.next_block_number() {
            tracing::error!(%block_number, %mined_number, "failed to save block because mismatch with mined block number");
            return Err(StorageError::MinedNumberConflict {
                new: block_number,
                mined: mined_number,
            });
        }

        // check pending number
        let pending_header = self.read_pending_block_header();
        if block_number >= pending_header.number {
            tracing::error!(%block_number, pending_number = %pending_header.number, "failed to save block because mismatch with pending block number");
            return Err(StorageError::PendingNumberConflict {
                new: block_number,
                pending: pending_header.number,
            });
        }

        // check mined block
        let existing_block = self.read_block(BlockFilter::Number(block_number))?;
        if existing_block.is_some() {
            tracing::error!(%block_number, %mined_number, "failed to save block because block with the same number already exists in the permanent storage");
            return Err(StorageError::BlockConflict { number: block_number });
        }

        // save block
        let (label_size_by_tx, label_size_by_gas) = (block.label_size_by_transactions(), block.label_size_by_gas());
        timed(|| self.perm.save_block(block)).with(|m| {
            metrics::inc_storage_save_block(m.elapsed, label::PERM, label_size_by_tx, label_size_by_gas, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, %block_number, "failed to save block");
            }
        })
    }

    pub fn save_block_batch(&self, blocks: Vec<Block>) -> Result<(), StratusError> {
        let Some(first) = blocks.first() else {
            tracing::error!("save_block_batch called with no blocks, ignoring");
            return Ok(());
        };

        let first_number = first.number();

        // check mined number
        let mined_number = self.read_mined_block_number()?;
        if not(first_number.is_zero()) && first_number != mined_number.next_block_number() {
            tracing::error!(%first_number, %mined_number, "failed to save block because mismatch with mined block number");
            return Err(StorageError::MinedNumberConflict {
                new: first_number,
                mined: mined_number,
            }
            .into());
        }

        // check pending number
        let pending_header = self.read_pending_block_header();
        if first_number >= pending_header.number {
            tracing::error!(%first_number, pending_number = %pending_header.number, "failed to save block because mismatch with pending block number");
            return Err(StorageError::PendingNumberConflict {
                new: first_number,
                pending: pending_header.number,
            }
            .into());
        }

        // check number of rest of blocks
        for window in blocks.windows(2) {
            let (previous, next) = (window[0].number(), window[1].number());
            if previous.next_block_number() != next {
                tracing::error!(%previous, %next, "previous block number doesn't match next one");
                return Err(anyhow!("consecutive blocks in batch aren't adjacent").into());
            }
        }

        // check mined block
        let existing_block = self.read_block(BlockFilter::Number(first_number))?;
        if existing_block.is_some() {
            tracing::error!(%first_number, %mined_number, "failed to save block because block with the same number already exists in the permanent storage");
            return Err(StorageError::BlockConflict { number: first_number }.into());
        }

        self.perm.save_block_batch(blocks).map_err(Into::into)
    }

    pub fn read_block(&self, filter: BlockFilter) -> Result<Option<Block>, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_block", %filter).entered();
        tracing::debug!(storage = %label::PERM, ?filter, "reading block");

        timed(|| self.perm.read_block(filter)).with(|m| {
            metrics::inc_storage_read_block(m.elapsed, label::PERM, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to read block");
            }
        })
    }

    pub fn read_transaction(&self, tx_hash: Hash) -> Result<Option<TransactionStage>, StorageError> {
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

    pub fn read_logs(&self, filter: &LogFilter) -> Result<Vec<LogMined>, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_logs", ?filter).entered();
        tracing::debug!(storage = %label::PERM, ?filter, "reading logs");

        timed(|| self.perm.read_logs(filter)).with(|m| {
            metrics::inc_storage_read_logs(m.elapsed, label::PERM, m.result.is_ok());
            if let Err(ref e) = m.result {
                tracing::error!(reason = ?e, "failed to read logs");
            }
        })
    }

    // -------------------------------------------------------------------------
    // General state
    // -------------------------------------------------------------------------

    #[cfg(feature = "dev")]
    /// Resets the storage to the genesis state used in dev-mode.
    ///
    /// TODO: For now it uses the dev genesis block and test accounts, but it should be refactored to support genesis.json files.
    pub fn reset_to_genesis(&self) -> Result<(), StorageError> {
        use crate::eth::primitives::test_accounts;

        self.cache.clear();

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

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Utils
    // -------------------------------------------------------------------------

    /// Translates a block filter to a specific storage point-in-time indicator.
    pub fn translate_to_point_in_time(&self, block_filter: BlockFilter) -> Result<PointInTime, StorageError> {
        match block_filter {
            BlockFilter::Pending => Ok(PointInTime::Pending),
            BlockFilter::Latest => Ok(PointInTime::Mined),
            BlockFilter::Earliest => Ok(PointInTime::MinedPast(BlockNumber::ZERO)),
            BlockFilter::Number(number) => Ok(PointInTime::MinedPast(number)),
            BlockFilter::Hash(_) => match self.read_block(block_filter)? {
                Some(block) => Ok(PointInTime::MinedPast(block.header.number)),
                None => Err(StorageError::BlockNotFound { filter: block_filter }),
            },
        }
    }

    /// Gets the RocksDB latest sequence number from the permanent storage
    pub fn get_rocksdb_latest_sequence_number(&self) -> Result<u64, StorageError> {
        self.perm.get_latest_sequence_number()
    }

    /// Gets the RocksDB WAL updates since the given sequence number
    pub fn get_rocksdb_updates_since(&self, seq_number: u64) -> Result<Vec<(u64, Vec<u8>)>, StorageError> {
        self.perm.get_updates_since(seq_number)
    }
}
