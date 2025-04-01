use anyhow::anyhow;
use tracing::Span;

use super::StorageCache;
#[cfg(feature = "dev")]
use crate::eth::genesis::GenesisConfig;
#[cfg(feature = "dev")]
use crate::eth::primitives::test_accounts;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
#[cfg(feature = "dev")]
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
#[cfg(feature = "dev")]
use crate::eth::primitives::Nonce;
use crate::eth::primitives::PendingBlock;
use crate::eth::primitives::PendingBlockHeader;
use crate::eth::primitives::PointInTime;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
#[cfg(feature = "dev")]
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::StorageError;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionStage;
#[cfg(feature = "dev")]
use crate::eth::primitives::Wei;
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
    #[cfg(feature = "dev")]
    perm_config: crate::eth::storage::permanent::PermanentStorageConfig,
}

impl StratusStorage {
    /// Creates a new storage with the specified temporary and permanent implementations.
    pub fn new(
        temp: Box<dyn TemporaryStorage>,
        perm: Box<dyn PermanentStorage>,
        cache: StorageCache,
        #[cfg(feature = "dev")] perm_config: crate::eth::storage::permanent::PermanentStorageConfig,
    ) -> Result<Self, StorageError> {
        let this = Self {
            temp,
            cache,
            perm,
            #[cfg(feature = "dev")]
            perm_config,
        };

        // create genesis block and accounts if necessary
        #[cfg(feature = "dev")]
        {
            if !this.has_genesis()? {
                this.reset_to_genesis()?;
            }
        }

        Ok(this)
    }

    pub fn has_genesis(&self) -> Result<bool, StorageError> {
        let genesis = self.read_block(BlockFilter::Number(BlockNumber::ZERO))?;
        Ok(genesis.is_some())
    }

    #[cfg(test)]
    pub fn new_test() -> Result<Self, StorageError> {
        use super::cache::CacheConfig;

        let temp = Box::new(super::InMemoryTemporaryStorage::new(0.into()));
        let perm = Box::new(super::InMemoryPermanentStorage::default());
        let cache = CacheConfig {
            slot_cache_capacity: 100000,
            account_cache_capacity: 20000,
        }
        .init();

        return Self::new(
            temp,
            perm,
            cache,
            #[cfg(feature = "dev")]
            super::permanent::PermanentStorageConfig {
                perm_storage_kind: super::permanent::PermanentStorageKind::InMemory,
                perm_storage_url: None,
                rocks_path_prefix: None,
                rocks_shutdown_timeout: std::time::Duration::from_secs(240),
                rocks_cache_size_multiplier: None,
                rocks_disable_sync_write: false,
                genesis_file: crate::config::GenesisFileConfig::default(),
            },
        );
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
    // Direct state manipulation (for testing)
    // -------------------------------------------------------------------------

    #[cfg(feature = "dev")]
    pub fn set_storage_at(&self, address: Address, index: SlotIndex, value: SlotValue) -> Result<(), StorageError> {
        // Create a slot with the given index and value
        let slot = Slot::new(index, value);

        // Update permanent storage
        self.perm.save_slot(address, slot)?;

        // Update temporary storage
        self.temp.save_slot(address, slot)?;

        // Update cache
        self.cache.cache_slot(address, slot);

        Ok(())
    }

    #[cfg(feature = "dev")]
    pub fn set_nonce(&self, address: Address, nonce: Nonce) -> Result<(), StorageError> {
        // Update permanent storage
        self.perm.save_account_nonce(address, nonce)?;

        // Update temporary storage
        self.temp.save_account_nonce(address, nonce)?;

        // Update cache
        let point_in_time = PointInTime::Mined;
        if let Some(account) = self.perm.read_account(address, point_in_time)? {
            self.cache.cache_account(account);
        }

        Ok(())
    }

    #[cfg(feature = "dev")]
    pub fn set_balance(&self, address: Address, balance: Wei) -> Result<(), StorageError> {
        // Update permanent storage
        self.perm.save_account_balance(address, balance)?;

        // Update temporary storage
        self.temp.save_account_balance(address, balance)?;

        // Update cache
        let point_in_time = PointInTime::Mined;
        if let Some(account) = self.perm.read_account(address, point_in_time)? {
            self.cache.cache_account(account);
        }

        Ok(())
    }

    #[cfg(feature = "dev")]
    pub fn set_code(&self, address: Address, code: Bytes) -> Result<(), StorageError> {
        // Update permanent storage
        self.perm.save_account_code(address, code.clone())?;

        // Update temporary storage
        self.temp.save_account_code(address, code)?;

        // Update cache
        let point_in_time = PointInTime::Mined;
        if let Some(account) = self.perm.read_account(address, point_in_time)? {
            self.cache.cache_account(account);
        }

        Ok(())
    }

    // -------------------------------------------------------------------------
    // General state
    // -------------------------------------------------------------------------

    #[cfg(feature = "dev")]
    /// Resets the storage to the genesis state.
    /// If a genesis.json file is available, it will be used.
    /// Otherwise, it will use the default genesis configuration.
    pub fn reset_to_genesis(&self) -> Result<(), StorageError> {
        tracing::info!("Resetting storage to genesis state");

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

        // Try to load genesis block from the genesis file or use default
        let genesis_block = if let Some(genesis_path) = &self.perm_config.genesis_file.genesis_path {
            if std::path::Path::new(genesis_path).exists() {
                match GenesisConfig::load_from_file(genesis_path) {
                    Ok(genesis_config) => match genesis_config.to_genesis_block() {
                        Ok(block) => {
                            tracing::info!("Using genesis block from file: {:?}", genesis_path);
                            block
                        }
                        Err(e) => {
                            tracing::error!("Failed to create genesis block from file: {:?}", e);
                            Block::genesis()
                        }
                    },
                    Err(e) => {
                        tracing::error!("Failed to load genesis file: {:?}", e);
                        Block::genesis()
                    }
                }
            } else {
                tracing::error!("Genesis file not found at: {:?}", genesis_path);
                Block::genesis()
            }
        } else {
            tracing::info!("Using default genesis block");
            Block::genesis()
        };
        // Try to load genesis.json from the path specified in GenesisFileConfig
        // or use default genesis configuration
        let (genesis_accounts, genesis_slots) = if let Some(genesis_path) = &self.perm_config.genesis_file.genesis_path {
            if std::path::Path::new(genesis_path).exists() {
                tracing::info!("Found genesis file at: {:?}", genesis_path);
                match GenesisConfig::load_from_file(genesis_path) {
                    Ok(genesis) => match genesis.to_stratus_accounts_and_slots() {
                        Ok((accounts, slots)) => {
                            tracing::info!("Loaded {} accounts from genesis.json", accounts.len());
                            if !slots.is_empty() {
                                tracing::info!("Loaded {} storage slots from genesis.json", slots.len());
                            }
                            (accounts, slots)
                        }
                        Err(e) => {
                            tracing::error!("Failed to convert genesis accounts: {:?}", e);
                            // Fallback to test accounts
                            (test_accounts(), vec![])
                        }
                    },
                    Err(e) => {
                        tracing::error!("Failed to load genesis file: {:?}", e);
                        // Fallback to test accounts
                        (test_accounts(), vec![])
                    }
                }
            } else {
                tracing::error!("Genesis file not found at: {:?}", genesis_path);
                // Fallback to test accounts
                (test_accounts(), vec![])
            }
        } else {
            // No genesis path specified, use default genesis configuration
            match GenesisConfig::default().to_stratus_accounts_and_slots() {
                Ok((accounts, slots)) => {
                    tracing::info!("Using default genesis configuration with {} accounts", accounts.len());
                    (accounts, slots)
                }
                Err(e) => {
                    tracing::error!("Failed to convert default genesis accounts: {:?}", e);
                    // Fallback to test accounts
                    (test_accounts(), vec![])
                }
            }
        };
        // Save the genesis block
        self.save_block(genesis_block)?;

        // accounts
        self.save_accounts(genesis_accounts)?;

        // Save slots if any
        if !genesis_slots.is_empty() {
            tracing::info!("Saving {} storage slots from genesis", genesis_slots.len());
            for (address, slot) in genesis_slots {
                self.perm.save_slot(address, slot)?;
            }
        }

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
}
