//! In-memory storage implementations.

use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::RwLockReadGuard;
use std::sync::RwLockWriteGuard;

use indexmap::IndexMap;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CodeHash;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::Wei;
use crate::eth::storage::inmemory::InMemoryHistory;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::StoragePointInTime;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct InMemoryPermanentStorageState {
    pub accounts: HashMap<Address, InMemoryPermanentAccount>,
    pub transactions: HashMap<Hash, TransactionMined>,
    pub blocks_by_number: IndexMap<BlockNumber, Arc<Block>>,
    pub blocks_by_hash: IndexMap<Hash, Arc<Block>>,
    pub logs: Vec<LogMined>,
}

#[derive(Debug)]
pub struct InMemoryPermanentStorage {
    state: RwLock<InMemoryPermanentStorageState>,
    block_number: AtomicU64,
}

impl InMemoryPermanentStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

impl InMemoryPermanentStorage {
    // -------------------------------------------------------------------------
    // Lock methods
    // -------------------------------------------------------------------------

    /// Locks inner state for reading.
    fn lock_read(&self) -> RwLockReadGuard<'_, InMemoryPermanentStorageState> {
        self.state.read().unwrap()
    }

    /// Locks inner state for writing.
    fn lock_write(&self) -> RwLockWriteGuard<'_, InMemoryPermanentStorageState> {
        self.state.write().unwrap()
    }

    // -------------------------------------------------------------------------
    // Snapshot methods
    // -------------------------------------------------------------------------

    /// Dump a snapshot of an execution previous state that can be used in tests.
    pub fn dump_snapshot(changes: Vec<ExecutionAccountChanges>) -> InMemoryPermanentStorageState {
        let mut state = InMemoryPermanentStorageState::default();
        for change in changes {
            // save account
            let mut account = InMemoryPermanentAccount::new_empty(change.address);
            account.balance = InMemoryHistory::new_at_zero(change.balance.take_original().unwrap_or_default());
            account.nonce = InMemoryHistory::new_at_zero(change.nonce.take_original().unwrap_or_default());
            account.bytecode = InMemoryHistory::new_at_zero(change.bytecode.take_original().unwrap_or_default());

            // save slots
            for (index, slot) in change.slots {
                let slot = slot.take_original().unwrap_or_default();
                let slot_history = InMemoryHistory::new_at_zero(slot);
                account.slots.insert(index, slot_history);
            }

            state.accounts.insert(change.address, account);
        }
        state
    }

    /// Creates a new InMemoryPermanentStorage from a snapshot dump.
    pub fn from_snapshot(state: InMemoryPermanentStorageState) -> Self {
        tracing::info!("creating inmemory permanent storage from snapshot");
        Self {
            state: RwLock::new(state),
            block_number: AtomicU64::new(0),
        }
    }

    // -------------------------------------------------------------------------
    // State methods
    // -------------------------------------------------------------------------

    /// Clears in-memory state.
    pub fn clear(&self) {
        let mut state = self.lock_write();
        state.accounts.clear();
        state.transactions.clear();
        state.blocks_by_hash.clear();
        state.blocks_by_number.clear();
        state.logs.clear();
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

    fn read_mined_block_number(&self) -> anyhow::Result<BlockNumber> {
        Ok(self.block_number.load(Ordering::SeqCst).into())
    }

    fn set_mined_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        self.block_number.store(number.as_u64(), Ordering::SeqCst);
        Ok(())
    }

    // -------------------------------------------------------------------------
    // State operations
    // ------------------------------------------------------------------------

    fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>> {
        let state = self.lock_read();

        match state.accounts.get(address) {
            Some(inmemory_account) => {
                let account = inmemory_account.to_account(point_in_time);
                Ok(Some(account))
            }
            None => Ok(None),
        }
    }

    fn read_slot(&self, address: &Address, index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        let state = self.lock_read();

        let Some(account) = state.accounts.get(address) else {
            return Ok(None);
        };

        match account.slots.get(index) {
            Some(slot_history) => {
                let slot = slot_history.get_at_point(point_in_time).unwrap_or_default();
                Ok(Some(slot))
            }
            None => Ok(None),
        }
    }

    fn read_block(&self, selection: &BlockFilter) -> anyhow::Result<Option<Block>> {
        let state_lock = self.lock_read();
        let block = match selection {
            BlockFilter::Latest | BlockFilter::Pending => state_lock.blocks_by_number.values().last().cloned(),
            BlockFilter::Earliest => state_lock.blocks_by_number.values().next().cloned(),
            BlockFilter::Number(block_number) => state_lock.blocks_by_number.get(block_number).cloned(),
            BlockFilter::Hash(block_hash) => state_lock.blocks_by_hash.get(block_hash).cloned(),
        };
        match block {
            Some(block) => Ok(Some((*block).clone())),
            None => Ok(None),
        }
    }

    fn read_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        let state_lock = self.lock_read();

        match state_lock.transactions.get(hash) {
            Some(transaction) => Ok(Some(transaction.clone())),
            None => Ok(None),
        }
    }

    fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        let state_lock = self.lock_read();

        let logs = state_lock
            .logs
            .iter()
            .skip_while(|log| log.block_number < filter.from_block)
            .take_while(|log| match filter.to_block {
                Some(to_block) => log.block_number <= to_block,
                None => true,
            })
            .filter(|log| filter.matches(log))
            .cloned()
            .collect();
        Ok(logs)
    }

    fn save_block(&self, block: Block) -> anyhow::Result<()> {
        let mut state = self.lock_write();

        // save block
        let block = Arc::new(block);
        let block_number = block.number();
        state.blocks_by_number.insert(block_number, Arc::clone(&block));
        state.blocks_by_hash.insert(block.hash(), Arc::clone(&block));

        // save transactions
        for transaction in block.transactions.clone() {
            state.transactions.insert(transaction.input.hash, transaction.clone());
            if transaction.is_success() {
                for log in transaction.logs {
                    state.logs.push(log);
                }
            }
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

    fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        let mut state = self.lock_write();
        for account in accounts {
            state
                .accounts
                .insert(account.address, InMemoryPermanentAccount::new_with_balance(account.address, account.balance));
        }
        Ok(())
    }

    #[cfg(feature = "dev")]
    fn reset(&self) -> anyhow::Result<()> {
        self.block_number.store(0u64, Ordering::SeqCst);

        let mut state = self.lock_write();
        *state = InMemoryPermanentStorageState::default();

        Ok(())
    }
}

/// TODO: group bytecode, code_hash, static_slot_indexes and mapping_slot_indexes into a single bytecode struct.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InMemoryPermanentAccount {
    #[allow(dead_code)]
    pub address: Address,
    pub balance: InMemoryHistory<Wei>,
    pub nonce: InMemoryHistory<Nonce>,
    pub bytecode: InMemoryHistory<Option<Bytes>>,
    pub code_hash: InMemoryHistory<CodeHash>,
    pub slots: HashMap<SlotIndex, InMemoryHistory<Slot>>,
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

    /// Resets all account changes to the specified block number.
    pub fn reset_at(&mut self, block_number: BlockNumber) {
        // SAFETY: ok to unwrap because all historical values starts at block 0
        self.balance = self.balance.reset_at(block_number).expect("never empty");
        self.nonce = self.nonce.reset_at(block_number).expect("never empty");
        self.bytecode = self.bytecode.reset_at(block_number).expect("never empty");

        // SAFETY: not ok to unwrap because slot value does not start at block 0
        let mut new_slots = HashMap::with_capacity(self.slots.len());
        for (slot_index, slot_history) in self.slots.iter() {
            if let Some(new_slot_history) = slot_history.reset_at(block_number) {
                new_slots.insert(*slot_index, new_slot_history);
            }
        }
        self.slots = new_slots;
    }

    /// Checks current account is a contract.
    pub fn is_contract(&self) -> bool {
        self.bytecode.get_current().is_some()
    }

    /// Converts itself to an account at a point-in-time.
    pub fn to_account(&self, point_in_time: &StoragePointInTime) -> Account {
        Account {
            address: self.address,
            balance: self.balance.get_at_point(point_in_time).unwrap_or_default(),
            nonce: self.nonce.get_at_point(point_in_time).unwrap_or_default(),
            bytecode: self.bytecode.get_at_point(point_in_time).unwrap_or_default(),
            code_hash: self.code_hash.get_at_point(point_in_time).unwrap_or_default(),
        }
    }
}
