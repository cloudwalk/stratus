//! In-memory storage implementations.

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use indexmap::IndexMap;
use itertools::Itertools;
use nonempty::NonEmpty;
use parking_lot::RwLock;
use parking_lot::RwLockReadGuard;
use parking_lot::RwLockWriteGuard;
use revm::primitives::Bytecode;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::CodeHash;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::PointInTime;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StorageError;
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::Wei;
use crate::eth::storage::PermanentStorage;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct InMemoryPermanentStorageState {
    pub accounts: HashMap<Address, InMemoryPermanentAccount, hash_hasher::HashBuildHasher>,
    pub transactions: HashMap<Hash, Arc<Block>, hash_hasher::HashBuildHasher>,
    pub blocks_by_number: IndexMap<BlockNumber, Arc<Block>>,
    pub blocks_by_hash: IndexMap<Hash, Arc<Block>>,
}

#[derive(Debug)]
pub struct InMemoryPermanentStorage {
    state: RwLock<InMemoryPermanentStorageState>,
    block_number: AtomicU64,
}

impl InMemoryPermanentStorage {
    // -------------------------------------------------------------------------
    // Lock methods
    // -------------------------------------------------------------------------

    /// Locks inner state for reading.
    fn lock_read(&self) -> RwLockReadGuard<'_, InMemoryPermanentStorageState> {
        self.state.read()
    }

    /// Locks inner state for writing.
    fn lock_write(&self) -> RwLockWriteGuard<'_, InMemoryPermanentStorageState> {
        self.state.write()
    }
}

impl Default for InMemoryPermanentStorage {
    fn default() -> Self {
        tracing::info!("creating inmemory permanent storage");
        Self {
            state: RwLock::new(InMemoryPermanentStorageState::default()),
            block_number: AtomicU64::default(),
        }
    }
}

impl PermanentStorage for InMemoryPermanentStorage {
    // -------------------------------------------------------------------------
    // Block number operations
    // -------------------------------------------------------------------------

    fn read_mined_block_number(&self) -> anyhow::Result<BlockNumber, StorageError> {
        Ok(self.block_number.load(Ordering::SeqCst).into())
    }

    fn set_mined_block_number(&self, number: BlockNumber) -> anyhow::Result<(), StorageError> {
        self.block_number.store(number.as_u64(), Ordering::SeqCst);
        Ok(())
    }

    // -------------------------------------------------------------------------
    // State operations
    // -------------------------------------------------------------------------

    fn read_account(&self, address: Address, point_in_time: PointInTime) -> anyhow::Result<Option<Account>, StorageError> {
        let state = self.lock_read();

        match state.accounts.get(&address) {
            Some(inmemory_account) => {
                let account = inmemory_account.to_account(point_in_time);
                Ok(Some(account))
            }
            None => Ok(None),
        }
    }

    fn read_slot(&self, address: Address, index: SlotIndex, point_in_time: PointInTime) -> anyhow::Result<Option<Slot>, StorageError> {
        let state = self.lock_read();

        let Some(account) = state.accounts.get(&address) else {
            return Ok(None);
        };

        match account.slots.get(&index) {
            Some(slot_history) => {
                let slot = slot_history.get_at_point(point_in_time).unwrap_or_default();
                Ok(Some(slot))
            }
            None => Ok(None),
        }
    }

    fn read_block(&self, selection: BlockFilter) -> anyhow::Result<Option<Block>, StorageError> {
        let state_lock = self.lock_read();
        let block = match selection {
            BlockFilter::Latest | BlockFilter::Pending => state_lock.blocks_by_number.values().last().cloned(),
            BlockFilter::Earliest => state_lock.blocks_by_number.values().next().cloned(),
            BlockFilter::Number(block_number) => state_lock.blocks_by_number.get(&block_number).cloned(),
            BlockFilter::Hash(block_hash) => state_lock.blocks_by_hash.get(&block_hash).cloned(),
        };
        match block {
            Some(block) => Ok(Some((*block).clone())),
            None => Ok(None),
        }
    }

    fn read_transaction(&self, hash: Hash) -> anyhow::Result<Option<TransactionMined>, StorageError> {
        let state_lock = self.lock_read();
        let Some(block) = state_lock.transactions.get(&hash) else { return Ok(None) };
        Ok(block.transactions.iter().find(|tx| tx.input.hash == hash).cloned())
    }

    fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>, StorageError> {
        let state = self.lock_read();

        // determine block start and end
        let start = filter.from_block.as_u64();
        let end = match filter.to_block {
            Some(to_block) => to_block.as_u64(),
            None => self.block_number.load(Ordering::Relaxed),
        };

        // iterate blocks filtering logs
        let mut filtered_logs = Vec::new();
        for block_number in start..=end {
            let block_number = BlockNumber::from(block_number);
            let Some(block) = state.blocks_by_number.get(&block_number) else {
                continue;
            };

            let tx_logs = block.transactions.iter().flat_map(|tx| &tx.logs).filter(|log| filter.matches(log));
            filtered_logs.extend(tx_logs);
        }

        Ok(filtered_logs.into_iter().cloned().collect_vec())
    }

    fn save_block(&self, block: Block) -> anyhow::Result<(), StorageError> {
        let mut state = self.lock_write();

        // save block
        let block = Arc::new(block);
        let block_number = block.number();
        state.blocks_by_number.insert(block_number, Arc::clone(&block));
        state.blocks_by_hash.insert(block.hash(), Arc::clone(&block));

        // save transactions
        for tx in &block.transactions {
            state.transactions.insert(tx.input.hash, Arc::clone(&block));
        }

        // save block account changes
        for changes in block.compact_account_changes() {
            let account = state
                .accounts
                .entry(changes.address)
                .or_insert_with(|| InMemoryPermanentAccount::new_empty(changes.address));

            // account basic info
            if let Some(nonce) = changes.nonce.take_modified() {
                account.nonce.push(block_number, nonce);
            }
            if let Some(balance) = changes.balance.take_modified() {
                account.balance.push(block_number, balance);
            }

            // bytecode
            if let Some(Some(bytecode)) = changes.bytecode.take_modified() {
                account.bytecode.push(block_number, Some(bytecode));
            }

            // slots
            for (_, slot) in changes.slots {
                if let Some(slot) = slot.take_modified() {
                    match account.slots.get_mut(&slot.index) {
                        Some(slot_history) => {
                            slot_history.push(block_number, slot);
                        }
                        None => {
                            account.slots.insert(slot.index, InMemoryHistory::new(block_number, slot));
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<(), StorageError> {
        let mut state = self.lock_write();
        for account in accounts {
            state
                .accounts
                .insert(account.address, InMemoryPermanentAccount::new_with_balance(account.address, account.balance));
        }
        Ok(())
    }

    #[cfg(feature = "dev")]
    fn reset(&self) -> anyhow::Result<(), StorageError> {
        self.block_number.store(0u64, Ordering::SeqCst);

        let mut state = self.lock_write();
        *state = InMemoryPermanentStorageState::default();

        Ok(())
    }

    fn get_latest_sequence_number(&self) -> anyhow::Result<u64, StorageError> {
        tracing::warn!("get_latest_sequence_number called on InMemoryPermanentStorage (no-op)");
        Ok(0)
    }

    fn get_updates_since(&self, _seq_number: u64) -> anyhow::Result<Vec<(u64, Vec<u8>)>, StorageError> {
        tracing::warn!("get_updates_since called on InMemoryPermanentStorage (no-op)");
        Ok(Vec::new())
    }

    fn apply_replication_logs(&self, _logs: Vec<(u64, Vec<u8>)>) -> anyhow::Result<(), StorageError> {
        tracing::warn!("apply_replication_logs called on InMemoryPermanentStorage (no-op)");
        Ok(())
    }

    fn create_checkpoint(&self, _checkpoint_dir: &std::path::Path) -> anyhow::Result<(), StorageError> {
        tracing::warn!("create_checkpoint called on InMemoryPermanentStorage (no-op)");
        Ok(())
    }
}

/// TODO: group bytecode, code_hash, static_slot_indexes and mapping_slot_indexes into a single bytecode struct.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct InMemoryPermanentAccount {
    #[allow(dead_code)]
    pub address: Address,
    pub balance: InMemoryHistory<Wei>,
    pub nonce: InMemoryHistory<Nonce>,
    pub bytecode: InMemoryHistory<Option<Bytecode>>,
    pub code_hash: InMemoryHistory<CodeHash>,
    pub slots: HashMap<SlotIndex, InMemoryHistory<Slot>, hash_hasher::HashBuildHasher>,
}

impl InMemoryPermanentAccount {
    /// Creates a new empty permanent account.
    fn new_empty(address: Address) -> Self {
        Self::new_with_balance(address, Wei::ZERO)
    }

    /// Creates a new permanent account with initial balance.
    pub fn new_with_balance(address: Address, balance: Wei) -> Self {
        Self {
            address,
            balance: InMemoryHistory::new_at_zero(balance),
            nonce: InMemoryHistory::new_at_zero(Nonce::ZERO),
            bytecode: InMemoryHistory::new_at_zero(None),
            code_hash: InMemoryHistory::new_at_zero(CodeHash::default()),
            slots: HashMap::default(),
        }
    }

    /// Converts itself to an account at a point-in-time.
    pub fn to_account(&self, point_in_time: PointInTime) -> Account {
        Account {
            address: self.address,
            balance: self.balance.get_at_point(point_in_time).unwrap_or_default(),
            nonce: self.nonce.get_at_point(point_in_time).unwrap_or_default(),
            bytecode: self.bytecode.get_at_point(point_in_time).unwrap_or_default(),
            code_hash: self.code_hash.get_at_point(point_in_time).unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct InMemoryHistory<T>(NonEmpty<InMemoryHistoryValue<T>>)
where
    T: Clone + Debug + serde::Serialize;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, derive_new::new)]
struct InMemoryHistoryValue<T> {
    pub block_number: BlockNumber,
    pub value: T,
}

impl<T> InMemoryHistory<T>
where
    T: Clone + Debug + serde::Serialize + for<'a> serde::Deserialize<'a>,
{
    /// Creates a new list of historical values.
    pub fn new_at_zero(value: T) -> Self {
        Self::new(BlockNumber::ZERO, value)
    }

    /// Creates a new list of historical values.
    pub fn new(block_number: BlockNumber, value: T) -> Self {
        let value = InMemoryHistoryValue::new(block_number, value);
        Self(NonEmpty::new(value))
    }

    /// Adds a new historical value to the list.
    pub fn push(&mut self, block_number: BlockNumber, value: T) {
        let value = InMemoryHistoryValue::new(block_number, value);
        self.0.push(value);
    }

    /// Returns the value at the given point in time.
    pub fn get_at_point(&self, point_in_time: PointInTime) -> Option<T> {
        match point_in_time {
            PointInTime::Mined | PointInTime::Pending => Some(self.get_current()),
            PointInTime::MinedPast(block_number) => self.get_at_block(block_number),
        }
    }

    /// Returns the most recent value before or at the given block number.
    pub fn get_at_block(&self, block_number: BlockNumber) -> Option<T> {
        self.0.iter().take_while(|x| x.block_number <= block_number).map(|x| &x.value).last().cloned()
    }

    /// Returns the most recent value.
    pub fn get_current(&self) -> T {
        self.0.last().value.clone()
    }
}
