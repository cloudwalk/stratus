use std::collections::HashMap;

use stratus_macros::timed;
use tracing::Span;

use crate::eth::executor::AccessListOutput;
use crate::eth::executor::State;
use crate::eth::executor::TransactionExecution;
use crate::eth::executor::types::state::AccountOriginalsReader;
use crate::eth::executor::types::state::Complete;
#[cfg(feature = "dev")]
use crate::eth::genesis::GenesisConfig;
use crate::eth::rpc::BlockFilter;
use crate::eth::rpc::LogFilter;
use crate::eth::storage::ExecutionKind;
use crate::eth::storage::InMemoryTemporaryStorage;
use crate::eth::storage::RocksPermanentStorage;
use crate::eth::storage::StorageCache;
use crate::eth::storage::StorageError;
use crate::eth::storage::permanent::rocks::types::BlockChangesRocksdb;
use crate::eth::storage::permanent::rocks::types::BlockRocksdb;
use crate::eth::storage::resolve_pending;
use crate::eth::storage::types::FoundAt;
use crate::eth::storage::types::entity::EntityRead;
use crate::eth::storage::types::state_lock::LatestStateLock;
use crate::eth::types::Account;
use crate::eth::types::Address;
use crate::eth::types::Block;
use crate::eth::types::BlockInfo;
use crate::eth::types::BlockNumber;
#[cfg(feature = "dev")]
use crate::eth::types::Bytes;
use crate::eth::types::ExternalBlock;
use crate::eth::types::Hash;
use crate::eth::types::LogMessage;
#[cfg(feature = "dev")]
use crate::eth::types::Nonce;
use crate::eth::types::PendingBlock;
use crate::eth::types::PointInTime;
use crate::eth::types::Slot;
use crate::eth::types::SlotIndex;
use crate::eth::types::SlotValue;
use crate::eth::types::TransactionStage;
use crate::eth::types::UnixTime;
#[cfg(feature = "dev")]
use crate::eth::types::Wei;
#[cfg(feature = "dev")]
use crate::eth::types::primitives::test_accounts;
use crate::ext::not;
use crate::infra::tracing::SpanExt;

pub mod label {
    pub const TEMP: &str = "temporary";
    pub const PERM: &str = "permanent";
}

/// Proxy that simplifies interaction with permanent and temporary storages.
///
/// Additionaly it tracks metrics that are independent of the storage implementation.
pub struct StratusStorage {
    pub temp: InMemoryTemporaryStorage,
    pub cache: StorageCache,
    pub perm: RocksPermanentStorage,
    // CONTRACT: Always acquire a lock when reading slots or accounts from latest (cache OR perm) and when saving a block.
    // The value in the lock is the latest block execution information.
    pub(super) latest_state_lock: LatestStateLock,
    #[cfg(feature = "dev")]
    perm_config: crate::eth::storage::permanent::PermanentStorageConfig,
}

impl AccountOriginalsReader for StratusStorage {
    fn read_accounts(&self, addresses: Vec<Address>) -> anyhow::Result<Vec<(Address, Account)>> {
        Ok(self.perm.read_accounts(addresses)?)
    }
}

pub use resolve_pending::MinedPointInTime;

impl StratusStorage {
    /// Creates a new storage with the specified temporary and permanent implementations.
    pub fn new(
        temp: InMemoryTemporaryStorage,
        perm: RocksPermanentStorage,
        cache: StorageCache,
        #[cfg(feature = "dev")] perm_config: crate::eth::storage::permanent::PermanentStorageConfig,
    ) -> Result<Self, StorageError> {
        let latest_block_info = perm.read_block(BlockFilter::Latest)?.map(|block| block.header.into()).unwrap_or_default();

        let this = Self {
            temp,
            cache,
            perm,
            latest_state_lock: LatestStateLock::new(latest_block_info),
            #[cfg(feature = "dev")]
            perm_config,
        };

        // create genesis block and accounts if necessary
        #[cfg(feature = "dev")]
        if !this.has_genesis()? {
            this.reset_to_genesis()?;
        }

        Ok(this)
    }

    /// Returns whether the genesis block exists
    pub fn has_genesis(&self) -> Result<bool, StorageError> {
        self.perm.has_genesis()
    }

    /// Clears the storage cache.
    pub fn clear_cache(&self) {
        tracing::info!("clearing storage cache");
        self.cache.clear();
    }

    #[cfg(test)]
    pub fn new_test() -> Result<Self, StorageError> {
        use tempfile::tempdir;

        use crate::eth::storage::cache::CacheConfig;

        let temp = InMemoryTemporaryStorage::new(0.into());

        // Create a temporary directory for RocksDB
        let rocks_dir = tempdir().expect("Failed to create temporary directory for tests");
        let rocks_path_prefix = rocks_dir.path().to_str().unwrap().to_string();

        let perm = RocksPermanentStorage::new(
            Some(rocks_path_prefix.clone()),
            std::time::Duration::from_secs(240),
            super::permanent::RocksCfCacheConfig::default(),
            true,
            None,
            1024,
        )
        .expect("Failed to create RocksPermanentStorage for tests");

        let cache = CacheConfig {
            account_history_cache_capacity: 20000,
            slot_history_cache_capacity: 100000,
        }
        .init();

        Self::new(
            temp,
            perm,
            cache,
            #[cfg(feature = "dev")]
            super::permanent::PermanentStorageConfig {
                rocks_path_prefix: Some(rocks_path_prefix),
                rocks_shutdown_timeout: std::time::Duration::from_secs(240),
                rocks_cf_cache: super::permanent::RocksCfCacheConfig::default(),
                rocks_disable_sync_write: false,
                rocks_cf_size_metrics_interval: None,
                genesis_file: crate::config::GenesisFileConfig::default(),
                rocks_file_descriptors_limit: 1024,
            },
        )
    }

    pub fn read_block_number_to_resume_import(&self) -> Result<BlockNumber, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_block_number_to_resume_import").entered();

        let number = self.read_pending_block_header().number;
        tracing::info!(?number, "got block number to resume import");

        Ok(number)
    }

    pub fn read_pending_block_header(&self) -> BlockInfo {
        self.temp.read_pending_block_header()
    }

    pub fn read_mined_block_number(&self) -> BlockNumber {
        self.perm.read_mined_block_number()
    }

    pub fn set_pending_from_external(&self, block: &ExternalBlock) {
        self.temp.set_pending_header(block.number(), block.timestamp());
    }

    pub fn set_pending_header(&self, number: BlockNumber, timestamp: UnixTime) {
        self.temp.set_pending_header(number, timestamp);
    }

    pub fn set_mined_block_number(&self, block_number: BlockNumber) {
        self.perm.set_mined_block_number(block_number);
    }

    // -------------------------------------------------------------------------
    // Accounts and slots
    // -------------------------------------------------------------------------

    #[cfg(feature = "dev")]
    pub fn save_accounts(&self, accounts: Vec<Account>) -> Result<(), StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::save_accounts").entered();

        // keep only accounts that does not exist in permanent storage
        let addresses: Vec<Address> = accounts.iter().map(|a| a.address).collect();
        let existing: std::collections::HashSet<Address> = self.perm.read_accounts(addresses)?.into_iter().map(|(addr, _)| addr).collect();
        let missing_accounts: Vec<Account> = accounts.into_iter().filter(|a| !existing.contains(&a.address)).collect();

        tracing::debug!(storage = %label::PERM, accounts = ?missing_accounts, "saving initial accounts");

        self.perm.save_accounts(missing_accounts)
    }

    /// Generic read algorithm shared by [`read_account`] and [`read_slot`].
    fn read<E: resolve_pending::Resolve>(&self, key: E::Key, kind: ExecutionKind) -> Result<(E, FoundAt), StorageError> {
        let (value, found_at) = 'query: {
            match E::resolve(self, key, kind) {
                resolve_pending::Resolved::Temp(value) => break 'query (value, FoundAt::Temp),
                resolve_pending::Resolved::Miss(mined_point) => {
                    let found_at = match &mined_point {
                        MinedPointInTime::Latest(_, _) =>
                        // Latest: try latest cache while guard is held, then fall through to perm.
                        {
                            let cached_value = if matches!(kind, ExecutionKind::AccessList) {
                                //bench without try_read
                                E::try_read_latest_cache(self, &key)
                            } else {
                                E::read_latest_cache(self, &key)
                            };
                            if let Some(value) = cached_value {
                                break 'query (value, FoundAt::Cache);
                            }
                            // If it wasnt found in the cache and we still have the guard the value can only be read in perm latest
                            FoundAt::PermLatest
                        }
                        MinedPointInTime::Past(_, _) => FoundAt::PermHistorical,
                    };
                    break 'query (E::read_perm(self, key, mined_point)?, found_at);
                }
            }
        };

        // Reads that held the transient state lock and were found at perm can be cached
        if matches!(
            (kind, found_at),
            (ExecutionKind::CallLatest(_) | ExecutionKind::CallPast(_), FoundAt::PermLatest)
        ) {
            E::cache_latest_if_missing(self, key, value.clone());
        }
        Ok((value, found_at))
    }

    pub fn read_account(&self, address: Address, kind: ExecutionKind) -> Result<(Account, FoundAt), StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("storage::read_account", %address).entered();
        self.read::<Account>(address, kind)
    }

    pub fn read_slot(&self, address: Address, index: SlotIndex, kind: ExecutionKind) -> Result<(Slot, FoundAt), StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::debug_span!("storage::read_slot", %address, %index).entered();
        self.read::<Slot>((address, index), kind)
    }

    // -------------------------------------------------------------------------
    // Blocks
    // -------------------------------------------------------------------------

    #[timed(storage_save_execution, labels(success = result.is_ok()))]
    pub fn save_execution(&self, tx: TransactionExecution, state: State<Complete>) -> Result<(), StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::save_execution", tx_hash = %tx.info.hash).entered();

        // Log warning if a failed transaction has slot changes
        if !tx.output.result.is_success() {
            let total_slot_changes: usize = state.slots.len();

            if total_slot_changes > 0 {
                tracing::warn!(?tx, "Failed transaction contains {} slot change(s)", total_slot_changes);
            }
        }

        self.temp.save_pending_execution(tx, state)
    }

    /// Retrieves pending transactions being mined.
    pub fn pending_transactions(&self) -> Vec<TransactionExecution> {
        self.temp.read_pending_executions()
    }

    #[timed(storage_finish_pending_block)]
    pub fn finish_pending_block(&self) -> (PendingBlock, State<Complete>) {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::finish_pending_block", block_number = tracing::field::Empty).entered();

        let result = self.temp.finish_pending_block();
        Span::with(|s| s.rec_str("block_number", &result.0.header.number));

        result
    }

    /// Save the block and apply changes. This function acquires a write lock to the latest state lock.
    #[timed(storage_save_block, labels(storage = label::PERM, tens_of_millions_gas_used = |block| block.header.gas_used.as_u64() / 10_000_000))]
    fn commit_changes(&self, block: Block, changes: State<Complete>) -> Result<(), StorageError> {
        let block_number = block.number();

        let mut guard = self.latest_state_lock.write();
        let block_info = (&block.header).into();
        self.perm.save_block(block, changes.finalize())?;
        guard.set_latest_block_info(block_info);
        self.cache.cache_account_and_slots_latest_from_changes(changes);
        self.set_mined_block_number(block_number);
        drop(guard);

        Ok(())
    }

    pub fn save_block(&self, block: Block, changes: State<Complete>) -> Result<(), StorageError> {
        let block_number = block.number();

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::save_block", block_number = %block.number()).entered();
        tracing::debug!(storage = %label::PERM, block_number = %block_number, transactions_len = %block.transactions.len(), ?changes, "saving block");

        // check mined number
        let mined_number = self.read_mined_block_number();
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

        self.commit_changes(block, changes)?;

        Ok(())
    }

    #[timed(storage_read_block, labels(storage = label::PERM, success = result.is_ok()))]
    pub fn read_block(&self, filter: BlockFilter) -> Result<Option<Block>, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_block", %filter).entered();

        self.perm
            .read_block(filter)
            .inspect_err(|err| tracing::error!(reason = ?err, "failed to read block"))
    }

    pub fn read_block_info(&self, filter: BlockFilter) -> Result<Option<BlockInfo>, StorageError> {
        let latest_state = self.latest_state_lock.read();
        let mined = latest_state.number;

        let reduced_filter = match filter {
            BlockFilter::Number(n) if n == mined.next_block_number() => BlockFilter::Pending,
            BlockFilter::Number(n) if n == mined => BlockFilter::Latest,
            filter => filter,
        };

        match reduced_filter {
            BlockFilter::Pending => Ok(Some(self.read_pending_block_header())),
            BlockFilter::Latest => Ok(Some(*latest_state)),
            filter => {
                drop(latest_state);
                Ok(self.read_block(filter)?.map(|block| block.header.into()))
            }
        }
    }

    #[timed(storage_read_block_with_changes, labels(storage = label::PERM, success = result.is_ok()))]
    pub fn read_block_with_changes(&self, filter: BlockFilter) -> Result<Option<(BlockRocksdb, BlockChangesRocksdb)>, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_block_with_changes", %filter).entered();

        self.perm
            .read_block_with_changes(filter)
            .inspect_err(|err| tracing::error!(reason = ?err, "failed to read block with changes"))
    }

    pub fn read_transaction(&self, tx_hash: Hash) -> Result<Option<TransactionStage>, StorageError> {
        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::read_transaction", %tx_hash).entered();

        let read_perm = || {
            {
                self.perm
                    .read_transaction(tx_hash)
                    .inspect_err(|err| {
                        tracing::error!(
                            reason = ?err,
                            "failed to read transaction from permanent storage"
                        );
                    })
                    .map(|tx| tx.map(TransactionStage::Mined))
            }
        };

        self.temp
            .read_pending_execution(tx_hash)
            .map_or_else(read_perm, |tx| Ok(Some(TransactionStage::Pending(tx))))
    }

    pub fn read_logs(&self, filter: &LogFilter) -> Result<Vec<LogMessage>, StorageError> {
        self.perm.read_logs(filter)
    }

    // -------------------------------------------------------------------------
    // Direct state manipulation (for testing)
    // -------------------------------------------------------------------------

    #[cfg(feature = "dev")]
    pub fn set_storage_at(&self, address: Address, index: SlotIndex, value: SlotValue) -> Result<(), StorageError> {
        // Create a slot with the given index and value
        let slot = Slot::new(index, value);
        self.cache.clear();

        // Update permanent storage
        self.perm.save_slot(address, slot)?;

        // Update temporary storage
        self.temp.save_slot(address, slot)?;

        Ok(())
    }

    #[cfg(feature = "dev")]
    pub fn set_nonce(&self, address: Address, nonce: Nonce) -> Result<(), StorageError> {
        self.cache.clear();

        // Update permanent storage
        self.perm.save_account_nonce(address, nonce)?;

        // Update temporary storage
        self.temp.save_account_nonce(address, nonce)?;

        Ok(())
    }

    #[cfg(feature = "dev")]
    pub fn set_balance(&self, address: Address, balance: Wei) -> Result<(), StorageError> {
        self.cache.clear();

        // Update permanent storage
        self.perm.save_account_balance(address, balance)?;

        // Update temporary storage
        self.temp.save_account_balance(address, balance)?;

        Ok(())
    }

    #[cfg(feature = "dev")]
    pub fn set_code(&self, address: Address, code: Bytes) -> Result<(), StorageError> {
        self.cache.clear();
        // Update permanent storage
        self.perm.save_account_code(address, code.clone())?;

        // Update temporary storage
        self.temp.save_account_code(address, code)?;

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
        tracing::info!("resetting storage to genesis state");

        self.cache.clear();

        #[cfg(feature = "tracing")]
        let _span = tracing::info_span!("storage::reset").entered();

        // reset perm
        tracing::debug!(storage = %label::PERM, "resetting permanent storage");
        self.perm.reset()?;

        // reset temp
        tracing::debug!(storage = %label::TEMP, "reseting temporary storage");
        self.temp.reset();

        // Try to load genesis block from the genesis file or use default
        let genesis_block = if let Some(genesis_path) = &self.perm_config.genesis_file.genesis_path {
            if std::path::Path::new(genesis_path).exists() {
                match GenesisConfig::load_from_file(genesis_path) {
                    Ok(genesis_config) => match genesis_config.to_genesis_block() {
                        Ok(block) => {
                            tracing::info!("using genesis block from file: {:?}", genesis_path);
                            block
                        }
                        Err(e) => {
                            tracing::error!("failed to create genesis block from file: {:?}", e);
                            Block::genesis()
                        }
                    },
                    Err(e) => {
                        tracing::error!("failed to load genesis file: {:?}", e);
                        Block::genesis()
                    }
                }
            } else {
                tracing::error!("genesis file not found at: {:?}", genesis_path);
                Block::genesis()
            }
        } else {
            tracing::info!("using default genesis block");
            Block::genesis()
        };
        // Try to load genesis.json from the path specified in GenesisFileConfig
        // or use default genesis configuration
        let (genesis_accounts, genesis_slots) = if let Some(genesis_path) = &self.perm_config.genesis_file.genesis_path {
            if std::path::Path::new(genesis_path).exists() {
                tracing::info!("found genesis file at: {:?}", genesis_path);
                match GenesisConfig::load_from_file(genesis_path) {
                    Ok(genesis) => match genesis.to_stratus_accounts_and_slots() {
                        Ok((accounts, slots)) => {
                            tracing::info!("loaded {} accounts from genesis.json", accounts.len());
                            if !slots.is_empty() {
                                tracing::info!("loaded {} storage slots from genesis.json", slots.len());
                            }
                            (accounts, slots)
                        }
                        Err(e) => {
                            tracing::error!("failed to convert genesis accounts: {:?}", e);
                            // Fallback to test accounts
                            (test_accounts(), vec![])
                        }
                    },
                    Err(e) => {
                        tracing::error!("failed to load genesis file: {:?}", e);
                        // Fallback to test accounts
                        (test_accounts(), vec![])
                    }
                }
            } else {
                tracing::error!("genesis file not found at: {:?}", genesis_path);
                // Fallback to test accounts
                (test_accounts(), vec![])
            }
        } else {
            // No genesis path specified, use default genesis configuration
            match GenesisConfig::default().to_stratus_accounts_and_slots() {
                Ok((accounts, slots)) => {
                    tracing::info!("using default genesis configuration with {} accounts", accounts.len());
                    (accounts, slots)
                }
                Err(e) => {
                    tracing::error!("failed to convert default genesis accounts: {:?}", e);
                    // Fallback to test accounts
                    (test_accounts(), vec![])
                }
            }
        };
        // Save the genesis block
        self.save_block(genesis_block, State::default())?;

        // accounts
        self.save_accounts(genesis_accounts)?;

        // Save slots if any
        if !genesis_slots.is_empty() {
            tracing::info!("saving {} storage slots from genesis", genesis_slots.len());
            for (address, slot) in genesis_slots {
                self.perm.save_slot(address, slot)?;
            }
        }

        // block number
        self.set_mined_block_number(BlockNumber::ZERO);

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Utils
    // -------------------------------------------------------------------------

    /// Translates a block filter to a specific storage point-in-time indicator.
    pub fn translate_to_point_in_time(&self, block_filter: BlockFilter) -> Result<PointInTime, StorageError> {
        match block_filter {
            BlockFilter::Pending => Ok(PointInTime::Pending),
            BlockFilter::Latest => Ok(PointInTime::Latest),
            BlockFilter::Earliest => Ok(PointInTime::Past(BlockNumber::ZERO)),
            // if number == latest (/pending) should we return PointInTime::Latest (/Pending) ?
            BlockFilter::Number(number) => Ok(PointInTime::Past(number)),
            BlockFilter::Hash(_) | BlockFilter::Timestamp(_) => self
                .read_block(block_filter)?
                .map(|b| PointInTime::Past(b.number()))
                .ok_or(StorageError::BlockNotFound { filter: block_filter }),
        }
    }

    pub fn translate_to_block_number(&self, block_filter: BlockFilter) -> Result<BlockNumber, StorageError> {
        match block_filter {
            BlockFilter::Pending => Ok(self.read_pending_block_header().number),
            BlockFilter::Latest => Ok(self.read_mined_block_number()),
            BlockFilter::Hash(_) | BlockFilter::Timestamp(_) => self
                .read_block(block_filter)?
                .map(|b| b.number())
                .ok_or(StorageError::BlockNotFound { filter: block_filter }),
            BlockFilter::Earliest => Ok(BlockNumber::ZERO),
            BlockFilter::Number(number) => Ok(number),
        }
    }

    fn load_slots_to_cache(&self, slots: Vec<(Address, SlotIndex)>) -> Result<(), StorageError> {
        let existing_slots: HashMap<(Address, SlotIndex), SlotValue> = self
            .perm
            .read_slots(slots.clone())
            .inspect_err(|err| tracing::error!(?err, "reading slots from perm failed"))?
            .into_iter()
            .collect();

        for (address, index) in slots {
            let value = existing_slots.get(&(address, index)).copied().unwrap_or_default();
            Slot::cache_latest_if_missing(self, (address, index), Slot { index, value });
        }

        Ok(())
    }

    fn load_accounts_to_cache(&self, addresses: Vec<Address>) -> Result<(), StorageError> {
        let existing_accounts: HashMap<Address, Account> = self
            .perm
            .read_accounts(addresses.clone())
            .inspect_err(|err| tracing::error!(?err, "reading accounts from perm failed"))?
            .into_iter()
            .collect();
        for address in addresses {
            let account = existing_accounts.get(&address).cloned().unwrap_or_default();
            Account::cache_latest_if_missing(self, address, account);
        }

        Ok(())
    }

    pub fn load_access_list(&self, access_list: AccessListOutput) {
        // can error
        let mut account_addresses = vec![];
        let mut slot_keys = vec![];
        for (address, slots) in access_list {
            account_addresses.push(address);
            for slot_index in slots {
                slot_keys.push((address, slot_index));
            }
        } // skip each step if prev empty
        Account::retain_missing_keys(self, &mut account_addresses);
        Slot::retain_missing_keys(self, &mut slot_keys);
        self.load_accounts_to_cache(account_addresses).ok();
        self.load_slots_to_cache(slot_keys).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eth::executor::ExecutionResult;
    use crate::eth::executor::TransactionExecutionInput;
    use crate::eth::executor::TransactionExecutionResult;
    use crate::eth::executor::types::state::AccountChanges;
    use crate::eth::executor::types::state::CompleteValue;
    use crate::eth::types::Signature;
    use crate::eth::types::SlotValue;
    use crate::eth::types::TransactionInfo;
    use crate::eth::types::TransactionInput;
    use crate::eth::types::Wei;

    /// Saves an execution applying `changes` to the pending block, without finishing it.
    fn save_execution(storage: &StratusStorage, changes: State<Complete>) {
        let header = storage.read_pending_block_header();
        let evm_input = TransactionExecutionInput::create(&TransactionInput::default(), header);

        let result = TransactionExecutionResult {
            result: ExecutionResult::Success,
            ..Default::default()
        };

        let tx = TransactionExecution::new(TransactionInfo::default(), Signature::default(), evm_input, result);
        storage.save_execution(tx, changes).expect("save execution");
    }

    /// Mines a block applying `changes`
    fn mine_block(storage: &StratusStorage, changes: State<Complete>) -> BlockNumber {
        save_execution(storage, changes);

        let (block, block_changes) = storage.finish_pending_block();
        storage.save_block(block.into(), block_changes).expect("save block");

        storage.read_mined_block_number()
    }

    /// An `eth_call` pinned to a block that is no longer the latest must read the historical
    /// state at its captured block, not the current latest state.
    #[test]
    fn read_slot_for_call_pinned_to_older_block_must_not_read_latest_state() {
        let storage = StratusStorage::new_test().expect("failed to build test storage");

        let address = Address::new([0xAA; 20]);
        let index = SlotIndex::ZERO;

        // Mine a block setting slot S = 100. The eth_call captures this block.
        let mut changes1 = State::default();
        changes1
            .slots
            .insert((address, index), CompleteValue::Changed(SlotValue::from([100u64, 0, 0, 0])));
        let call_block = mine_block(&storage, changes1);

        // A new block is mined while the call is in flight, changing the slot to 200.
        let mut changes2 = State::default();
        changes2
            .slots
            .insert((address, index), CompleteValue::Changed(SlotValue::from([200u64, 0, 0, 0])));
        let latest = mine_block(&storage, changes2);
        assert_ne!(call_block, latest);

        // The in-flight call (pinned to the first block) reads the slot.
        let (slot, _) = storage.read_slot(address, index, ExecutionKind::CallLatest(call_block)).expect("read slot");

        // Must reflect the first block (100), not the freshly mined latest (200).
        assert_eq!(slot.value, SlotValue::from([100u64, 0, 0, 0]));
    }

    #[test]
    fn read_account_for_call_pinned_to_older_block_must_not_read_latest_state() {
        let storage = StratusStorage::new_test().expect("failed to build test storage");

        let address = Address::new([0xBB; 20]);

        // Mine a block setting the account balance to 100. The eth_call captures this block.
        let mut changes1 = State::default();
        changes1
            .accounts
            .insert(address, AccountChanges::from_changed(Account::new_with_balance(address, Wei::from(100u64))));
        let call_block = mine_block(&storage, changes1);

        // A new block is mined while the call is in flight, changing the balance to 200.
        let mut changes2 = State::default();
        changes2
            .accounts
            .insert(address, AccountChanges::from_changed(Account::new_with_balance(address, Wei::from(200u64))));
        let latest = mine_block(&storage, changes2);
        assert_ne!(call_block, latest);

        let (account, _) = storage.read_account(address, ExecutionKind::CallLatest(call_block)).expect("read account");

        // Must reflect the first block (100), not the freshly mined latest (200).
        assert_eq!(account.balance, Wei::from(100u64));
    }
}
