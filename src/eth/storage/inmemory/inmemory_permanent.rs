//! In-memory storage implementations.

use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use async_trait::async_trait;
use indexmap::IndexMap;
use rand::rngs::StdRng;
use rand::seq::IteratorRandom;
use rand::SeedableRng;
use tokio::sync::RwLock;
use tokio::sync::RwLockReadGuard;
use tokio::sync::RwLockWriteGuard;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::CodeHash;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::ExecutionConflicts;
use crate::eth::primitives::ExecutionConflictsBuilder;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotSample;
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::Wei;
use crate::eth::storage::inmemory::InMemoryHistory;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::StorageError;

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
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
    // -------------------------------------------------------------------------
    // Lock methods
    // -------------------------------------------------------------------------

    /// Locks inner state for reading.
    async fn lock_read(&self) -> RwLockReadGuard<'_, InMemoryPermanentStorageState> {
        self.state.read().await
    }

    /// Locks inner state for writing.
    async fn lock_write(&self) -> RwLockWriteGuard<'_, InMemoryPermanentStorageState> {
        self.state.write().await
    }

    // -------------------------------------------------------------------------
    // Snapshot methods
    // -------------------------------------------------------------------------

    /// Dump a snapshot of an execution previous state that can be used in tests.
    pub fn dump_snapshot(changes: Vec<ExecutionAccountChanges>) -> InMemoryPermanentStorageState {
        let mut state = InMemoryPermanentStorageState::default();
        for change in changes {
            // save account
            let mut account = InMemoryPermanentAccount::new_empty(change.address.clone());
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
        Self {
            state: RwLock::new(state),
            block_number: AtomicU64::new(0),
        }
    }

    // -------------------------------------------------------------------------
    // State methods
    // -------------------------------------------------------------------------

    /// Clears in-memory state.
    pub async fn clear(&self) {
        let mut state = self.lock_write().await;
        state.accounts.clear();
        state.transactions.clear();
        state.blocks_by_hash.clear();
        state.blocks_by_number.clear();
        state.logs.clear();
    }

    async fn check_conflicts(state: &InMemoryPermanentStorageState, account_changes: &[ExecutionAccountChanges]) -> Option<ExecutionConflicts> {
        let mut conflicts = ExecutionConflictsBuilder::default();

        for change in account_changes {
            let address = &change.address;

            if let Some(account) = state.accounts.get(address) {
                // check account info conflicts
                if let Some(original_nonce) = change.nonce.take_original_ref() {
                    let account_nonce = account.nonce.get_current_ref();
                    if original_nonce != account_nonce {
                        conflicts.add_nonce(address.clone(), account_nonce.clone(), original_nonce.clone());
                    }
                }
                if let Some(original_balance) = change.balance.take_original_ref() {
                    let account_balance = account.balance.get_current_ref();
                    if original_balance != account_balance {
                        conflicts.add_balance(address.clone(), account_balance.clone(), original_balance.clone());
                    }
                }

                // check slots conflicts
                for (slot_index, slot_change) in &change.slots {
                    if let Some(account_slot) = account.slots.get(slot_index).map(|value| value.get_current_ref()) {
                        if let Some(original_slot) = slot_change.take_original_ref() {
                            let account_slot_value = account_slot.value.clone();
                            if original_slot.value != account_slot_value {
                                conflicts.add_slot(address.clone(), slot_index.clone(), account_slot_value, original_slot.value.clone());
                            }
                        }
                    }
                }
            }
        }
        conflicts.build()
    }
}

impl Default for InMemoryPermanentStorage {
    fn default() -> Self {
        tracing::info!("starting inmemory permanent storage");
        Self {
            state: RwLock::new(InMemoryPermanentStorageState::default()),
            block_number: Default::default(),
        }
    }
}

#[async_trait]
impl PermanentStorage for InMemoryPermanentStorage {
    async fn allocate_evm_thread_resources(&self) -> anyhow::Result<()> {
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Block number operations
    // -------------------------------------------------------------------------

    async fn read_mined_block_number(&self) -> anyhow::Result<BlockNumber> {
        Ok(self.block_number.load(Ordering::SeqCst).into())
    }

    async fn increment_block_number(&self) -> anyhow::Result<BlockNumber> {
        let next = self.block_number.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(next.into())
    }

    async fn set_mined_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        self.block_number.store(number.as_u64(), Ordering::SeqCst);
        Ok(())
    }

    // -------------------------------------------------------------------------
    // State operations
    // ------------------------------------------------------------------------

    async fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>> {
        tracing::debug!(%address, "reading account");

        let state = self.lock_read().await;

        match state.accounts.get(address) {
            Some(inmemory_account) => {
                let account = inmemory_account.to_account(point_in_time);
                tracing::trace!(%address, ?account, "account found");
                Ok(Some(account))
            }

            None => {
                tracing::trace!(%address, "account not found");
                Ok(None)
            }
        }
    }

    async fn read_slot(&self, address: &Address, index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        tracing::debug!(%address, %index, ?point_in_time, "reading slot");

        let state = self.lock_read().await;
        let Some(account) = state.accounts.get(address) else {
            tracing::trace!(%address, "account not found");
            return Ok(Default::default());
        };

        match account.slots.get(index) {
            Some(slot_history) => {
                let slot = slot_history.get_at_point(point_in_time).unwrap_or_default();
                tracing::trace!(%address, %index, ?point_in_time, %slot, "slot found");
                Ok(Some(slot))
            }

            None => {
                tracing::trace!(%address, %index, ?point_in_time, "slot not found");
                Ok(None)
            }
        }
    }

    async fn read_slots(&self, address: &Address, indexes: &[SlotIndex], point_in_time: &StoragePointInTime) -> anyhow::Result<HashMap<SlotIndex, SlotValue>> {
        tracing::debug!(%address, indexes_len = %indexes.len(), "reading slots");

        let mut slots = HashMap::with_capacity(indexes.len());
        for index in indexes {
            let slot = self.read_slot(address, index, point_in_time).await?;
            if let Some(slot) = slot {
                slots.insert(slot.index, slot.value);
            }
        }

        Ok(slots)
    }

    async fn read_block(&self, selection: &BlockSelection) -> anyhow::Result<Option<Block>> {
        tracing::debug!(?selection, "reading block");

        let state_lock = self.lock_read().await;
        let block = match selection {
            BlockSelection::Latest => state_lock.blocks_by_number.values().last().cloned(),
            BlockSelection::Earliest => state_lock.blocks_by_number.values().next().cloned(),
            BlockSelection::Number(number) => state_lock.blocks_by_number.get(number).cloned(),
            BlockSelection::Hash(hash) => state_lock.blocks_by_hash.get(hash).cloned(),
        };
        match block {
            Some(block) => {
                tracing::trace!(?selection, ?block, "block found");
                Ok(Some((*block).clone()))
            }
            None => {
                tracing::trace!(?selection, "block not found");
                Ok(None)
            }
        }
    }

    async fn read_mined_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        tracing::debug!(%hash, "reading transaction");
        let state_lock = self.lock_read().await;

        match state_lock.transactions.get(hash) {
            Some(transaction) => {
                tracing::trace!(%hash, "transaction found");
                Ok(Some(transaction.clone()))
            }
            None => {
                tracing::trace!(%hash, "transaction not found");
                Ok(None)
            }
        }
    }

    async fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        tracing::debug!(?filter, "reading logs");
        let state_lock = self.lock_read().await;

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

    async fn save_block(&self, block: Block) -> anyhow::Result<(), StorageError> {
        let mut state = self.lock_write().await;

        // check conflicts before persisting any state changes
        let account_changes = block.compact_account_changes();
        if let Some(conflicts) = Self::check_conflicts(&state, &account_changes).await {
            return Err(StorageError::Conflict(conflicts));
        }

        // save block
        tracing::debug!(number = %block.number(), "saving block");
        let block = Arc::new(block);
        let number = block.number();
        state.blocks_by_number.insert(*number, Arc::clone(&block));
        state.blocks_by_hash.insert(block.hash().clone(), Arc::clone(&block));

        // save transactions
        for transaction in block.transactions.clone() {
            tracing::debug!(hash = %transaction.input.hash, "saving transaction");
            state.transactions.insert(transaction.input.hash.clone(), transaction.clone());
            if transaction.is_success() {
                for log in transaction.logs {
                    state.logs.push(log);
                }
            }
        }

        // save block account changes
        for changes in account_changes {
            let account = state
                .accounts
                .entry(changes.address.clone())
                .or_insert_with(|| InMemoryPermanentAccount::new_empty(changes.address));

            // account basic info
            if let Some(nonce) = changes.nonce.take_modified() {
                account.nonce.push(*number, nonce);
            }
            if let Some(balance) = changes.balance.take_modified() {
                account.balance.push(*number, balance);
            }

            // bytecode
            if let Some(Some(bytecode)) = changes.bytecode.take_modified() {
                account.bytecode.push(*number, Some(bytecode));
            }

            // slots
            for (_, slot) in changes.slots {
                if let Some(slot) = slot.take_modified() {
                    match account.slots.get_mut(&slot.index) {
                        Some(slot_history) => {
                            slot_history.push(*number, slot);
                        }
                        None => {
                            account.slots.insert(slot.index.clone(), InMemoryHistory::new(*number, slot));
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        tracing::debug!(?accounts, "saving initial accounts");

        let mut state = self.lock_write().await;
        for account in accounts {
            state.accounts.insert(
                account.address.clone(),
                InMemoryPermanentAccount::new_with_balance(account.address, account.balance),
            );
        }
        Ok(())
    }

    async fn reset_at(&self, block_number: BlockNumber) -> anyhow::Result<()> {
        // reset block number
        let block_number_u64: u64 = block_number.into();
        let _ = self.block_number.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
            if block_number_u64 <= current {
                Some(block_number_u64)
            } else {
                None
            }
        });

        // remove blocks
        let mut state = self.lock_write().await;
        state.blocks_by_hash.retain(|_, b| *b.number() <= block_number);
        state.blocks_by_number.retain(|_, b| *b.number() <= block_number);

        // remove transactions and logs
        state.transactions.retain(|_, t| t.block_number <= block_number);
        state.logs.retain(|l| l.block_number <= block_number);

        // remove account changes
        for account in state.accounts.values_mut() {
            account.reset_at(block_number);
        }

        Ok(())
    }

    async fn read_slots_sample(&self, start: BlockNumber, end: BlockNumber, max_samples: u64, seed: u64) -> anyhow::Result<Vec<SlotSample>> {
        let state = self.lock_read().await;

        let samples = state
            .accounts
            .iter()
            .filter(|(_, account_info)| account_info.is_contract())
            .flat_map(|(_, contract)| {
                contract
                    .slots
                    .values()
                    .flat_map(|slot_history| Vec::from((*slot_history).clone()))
                    .filter_map(|slot| {
                        if slot.block_number >= start && slot.block_number < end {
                            Some(SlotSample {
                                address: contract.address.clone(),
                                block_number: slot.block_number,
                                index: slot.value.index,
                                value: slot.value.value,
                            })
                        } else {
                            None
                        }
                    })
            });

        match max_samples {
            0 => Ok(samples.collect()),
            n => {
                let mut rng = StdRng::seed_from_u64(seed);
                Ok(samples.choose_multiple(&mut rng, n as usize))
            }
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct InMemoryPermanentAccount {
    #[allow(dead_code)]
    pub address: Address,
    pub balance: InMemoryHistory<Wei>,
    pub nonce: InMemoryHistory<Nonce>,
    pub bytecode: InMemoryHistory<Option<Bytes>>,
    pub code_hash: CodeHash,
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
            code_hash: CodeHash::default(),
            slots: Default::default(),
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
                new_slots.insert(slot_index.clone(), new_slot_history);
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
            address: self.address.clone(),
            balance: self.balance.get_at_point(point_in_time).unwrap_or_default(),
            nonce: self.nonce.get_at_point(point_in_time).unwrap_or_default(),
            bytecode: self.bytecode.get_at_point(point_in_time).unwrap_or_default(),
            code_hash: self.code_hash.clone(),
            slot_indexes_static_access: None,  // TODO: is it necessary for InMemory?
            slot_indexes_mapping_access: None, // TODO: is it necessary for InMemory?
        }
    }
}
