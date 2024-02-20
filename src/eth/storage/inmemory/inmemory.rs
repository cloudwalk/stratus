//! In-memory storage implementations.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use async_trait::async_trait;
use indexmap::IndexMap;
use metrics::atomics::AtomicU64;
use tokio::sync::RwLock;
use tokio::sync::RwLockReadGuard;
use tokio::sync::RwLockWriteGuard;

use super::InMemoryAccount;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Execution;
use crate::eth::primitives::ExecutionConflicts;
use crate::eth::primitives::ExecutionConflictsBuilder;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::inmemory::InMemoryHistory;
use crate::eth::storage::EthStorageError;
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::TemporaryStorage;

/// In-memory implementation using maps.
#[derive(Debug)]
pub struct InMemoryStorage {
    state: RwLock<InMemoryStorageState>,
    block_number: AtomicU64,
}

impl InMemoryStorage {
    /// Locks inner state for reading.
    async fn lock_read(&self) -> RwLockReadGuard<'_, InMemoryStorageState> {
        self.state.read().await
    }

    /// Locks inner state for writing.
    async fn lock_write(&self) -> RwLockWriteGuard<'_, InMemoryStorageState> {
        self.state.write().await
    }

    /// Flushes in-memory state
    pub async fn flush(&self) {
        let mut state = self.lock_write().await;
        state.accounts.clear();
        state.transactions.clear();
        state.blocks_by_hash.clear();
        state.blocks_by_number.clear();
        state.logs.clear();

        self.block_number.store(0, Ordering::SeqCst);
    }
}

#[derive(Debug, Default)]
struct InMemoryStorageState {
    accounts: HashMap<Address, InMemoryAccount>,
    transactions: HashMap<Hash, TransactionMined>,
    blocks_by_number: IndexMap<BlockNumber, Arc<Block>>,
    blocks_by_hash: IndexMap<Hash, Arc<Block>>,
    logs: Vec<LogMined>,
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        tracing::info!("starting inmemory storage");

        Self {
            state: RwLock::new(InMemoryStorageState::default()),
            block_number: Default::default(),
        }
    }
}

#[async_trait]
impl PermanentStorage for InMemoryStorage {
    // -------------------------------------------------------------------------
    // Block number operations
    // -------------------------------------------------------------------------

    async fn read_current_block_number(&self) -> anyhow::Result<BlockNumber> {
        Ok(self.block_number.load(Ordering::SeqCst).into())
    }

    // -------------------------------------------------------------------------
    // State operations
    // ------------------------------------------------------------------------

    async fn check_conflicts(&self, execution: &Execution) -> anyhow::Result<Option<ExecutionConflicts>> {
        let state_lock = self.state.read().await;
        Ok(check_conflicts(&state_lock, execution))
    }

    async fn maybe_read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>> {
        tracing::debug!(%address, "reading account");

        let state = self.lock_read().await;

        match state.accounts.get(address) {
            Some(account) => {
                let account = Account {
                    address: address.clone(),
                    balance: account.balance.get_at_point(point_in_time).unwrap_or_default(),
                    nonce: account.nonce.get_at_point(point_in_time).unwrap_or_default(),
                    bytecode: account.bytecode.get_at_point(point_in_time).unwrap_or_default(),
                };
                tracing::trace!(%address, ?account, "account found");
                Ok(Some(account))
            }

            None => {
                tracing::trace!(%address, "account not found");
                Ok(None)
            }
        }
    }

    async fn maybe_read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        tracing::debug!(%address, %slot_index, ?point_in_time, "reading slot");

        let state = self.lock_read().await;
        let Some(account) = state.accounts.get(address) else {
            tracing::trace!(%address, "account not found");
            return Ok(Default::default());
        };

        match account.slots.get(slot_index) {
            Some(slot_history) => {
                let slot = slot_history.get_at_point(point_in_time).unwrap_or_default();
                tracing::trace!(%address, %slot_index, ?point_in_time, %slot, "slot found");
                Ok(Some(slot))
            }

            None => {
                tracing::trace!(%address, %slot_index, ?point_in_time, "slot not found");
                Ok(None)
            }
        }
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

    async fn save_block(&self, block: Block) -> anyhow::Result<(), EthStorageError> {
        let mut state = self.lock_write().await;

        // check conflicts
        for transaction in &block.transactions {
            if let Some(conflicts) = check_conflicts(&state, &transaction.execution) {
                return Err(EthStorageError::Conflict(conflicts));
            }
        }

        // save block
        tracing::debug!(number = %block.number(), "saving block");
        let block = Arc::new(block);
        state.blocks_by_number.insert(*block.number(), Arc::clone(&block));
        state.blocks_by_hash.insert(block.hash().clone(), Arc::clone(&block));

        // save transactions
        for transaction in block.transactions.clone() {
            tracing::debug!(hash = %transaction.input.hash, "saving transaction");
            state.transactions.insert(transaction.input.hash.clone(), transaction.clone());
            let is_success = transaction.is_success();

            // save logs
            if is_success {
                for log in transaction.logs {
                    state.logs.push(log);
                }
            }

            // save execution changes
            save_account_changes(&mut state, *block.number(), transaction.execution);
        }
        Ok(())
    }

    async fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        tracing::debug!(?accounts, "saving initial accounts");

        let mut state = self.lock_write().await;
        for account in accounts {
            state
                .accounts
                .insert(account.address.clone(), InMemoryAccount::new_with_balance(account.address, account.balance));
        }
        Ok(())
    }

    async fn save_account_changes(&self, number: BlockNumber, execution: Execution) -> anyhow::Result<()> {
        let mut state_lock = self.lock_write().await;
        save_account_changes(&mut state_lock, number, execution);
        Ok(())
    }

    async fn reset(&self, block_number: BlockNumber) -> anyhow::Result<()> {
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
            account.reset(block_number);
        }

        Ok(())
    }

    async fn enable_genesis(&self, genesis: Block) -> anyhow::Result<()> {
        let block = Arc::new(genesis);
        let mut state = self.lock_write().await;
        state.blocks_by_number.insert(*block.number(), Arc::clone(&block));
        state.blocks_by_hash.insert(block.hash().clone(), block);
        Ok(())
    }
}

#[async_trait]
impl TemporaryStorage for InMemoryStorage {
    // -------------------------------------------------------------------------
    // Block number operations
    // -------------------------------------------------------------------------

    async fn read_current_block_number(&self) -> anyhow::Result<BlockNumber> {
        Ok(self.block_number.load(Ordering::SeqCst).into())
    }

    async fn increment_block_number(&self) -> anyhow::Result<BlockNumber> {
        let next = self.block_number.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(next.into())
    }

    // -------------------------------------------------------------------------
    // State operations
    // ------------------------------------------------------------------------

    async fn check_conflicts(&self, execution: &Execution) -> anyhow::Result<Option<ExecutionConflicts>> {
        let state_lock = self.state.read().await;
        Ok(check_conflicts(&state_lock, execution))
    }

    async fn maybe_read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>> {
        tracing::debug!(%address, "reading account");

        let state = self.lock_read().await;

        match state.accounts.get(address) {
            Some(account) => {
                let account = Account {
                    address: address.clone(),
                    balance: account.balance.get_at_point(point_in_time).unwrap_or_default(),
                    nonce: account.nonce.get_at_point(point_in_time).unwrap_or_default(),
                    bytecode: account.bytecode.get_at_point(point_in_time).unwrap_or_default(),
                };
                tracing::trace!(%address, ?account, "account found");
                Ok(Some(account))
            }

            None => {
                tracing::trace!(%address, "account not found");
                Ok(None)
            }
        }
    }

    async fn maybe_read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        tracing::debug!(%address, %slot_index, ?point_in_time, "reading slot");

        let state = self.lock_read().await;
        let Some(account) = state.accounts.get(address) else {
            tracing::trace!(%address, "account not found");
            return Ok(Default::default());
        };

        match account.slots.get(slot_index) {
            Some(slot_history) => {
                let slot = slot_history.get_at_point(point_in_time).unwrap_or_default();
                tracing::trace!(%address, %slot_index, ?point_in_time, %slot, "slot found");
                Ok(Some(slot))
            }

            None => {
                tracing::trace!(%address, %slot_index, ?point_in_time, "slot not found");
                Ok(None)
            }
        }
    }
    
    async fn save_block(&self, block: Block) -> anyhow::Result<(), EthStorageError> {
        let mut state = self.lock_write().await;

        // check conflicts
        for transaction in &block.transactions {
            if let Some(conflicts) = check_conflicts(&state, &transaction.execution) {
                return Err(EthStorageError::Conflict(conflicts));
            }
        }

        // save block
        tracing::debug!(number = %block.number(), "saving block");
        let block = Arc::new(block);
        state.blocks_by_number.insert(*block.number(), Arc::clone(&block));
        state.blocks_by_hash.insert(block.hash().clone(), Arc::clone(&block));

        // save transactions
        for transaction in block.transactions.clone() {
            tracing::debug!(hash = %transaction.input.hash, "saving transaction");
            state.transactions.insert(transaction.input.hash.clone(), transaction.clone());
            let is_success = transaction.is_success();

            // save logs
            if is_success {
                for log in transaction.logs {
                    state.logs.push(log);
                }
            }

            // save execution changes
            save_account_changes(&mut state, *block.number(), transaction.execution);
        }
        Ok(())
    }

    /// Commits changes to permanent storage and flushes overlay storage
    /// Basically calls the `save_block` method from the permanent storage, which
    /// will by definition update accounts, slots, transactions, logs etc
    async fn commit(&self, block: Block) -> anyhow::Result<(), EthStorageError> {
        PermanentStorage::save_block(self, block).await?;
        TemporaryStorage::reset(self).await?;

        Ok(())
    }

    async fn save_account_changes(&self, number: BlockNumber, execution: Execution) -> anyhow::Result<()> {
        let mut state_lock = self.lock_write().await;
        save_account_changes(&mut state_lock, number, execution);
        Ok(())
    }

    async fn reset(&self) -> anyhow::Result<()> {
        self.flush().await;
        Ok(())
    }
}

fn save_account_changes(state: &mut InMemoryStorageState, block_number: BlockNumber, execution: Execution) {
    let is_success = execution.is_success();
    for changes in execution.changes {
        let account = state
            .accounts
            .entry(changes.address.clone())
            .or_insert_with(|| InMemoryAccount::new(changes.address));

        // nonce
        if let Some(nonce) = changes.nonce.take_modified() {
            account.set_nonce(block_number, nonce);
        }

        // balance
        if let Some(balance) = changes.balance.take_modified() {
            account.set_balance(block_number, balance);
        }

        // bytecode
        if is_success {
            if let Some(Some(bytecode)) = changes.bytecode.take_modified() {
                account.set_bytecode(block_number, bytecode);
            }
        }

        // slots
        if is_success {
            for (slot_index, slot) in changes.slots {
                if let Some(slot) = slot.take_modified() {
                    match account.slots.get_mut(&slot_index) {
                        Some(slot_history) => {
                            slot_history.push(block_number, slot);
                        }
                        None => {
                            account.slots.insert(slot_index, InMemoryHistory::new(block_number, slot));
                        }
                    };
                }
            }
        }
    }
}

fn check_conflicts(state: &InMemoryStorageState, execution: &Execution) -> Option<ExecutionConflicts> {
    let mut conflicts = ExecutionConflictsBuilder::default();

    for change in &execution.changes {
        let address = &change.address;

        if let Some(account) = state.accounts.get(address) {
            // check account info conflicts
            if let Some(touched_nonce) = change.nonce.take_original_ref() {
                if touched_nonce != account.nonce.get_current_ref() {
                    conflicts.add_nonce(address.clone(), account.nonce.get_current(), touched_nonce.clone());
                }
            }
            if let Some(touched_balance) = change.balance.take_original_ref() {
                if touched_balance != account.balance.get_current_ref() {
                    conflicts.add_balance(address.clone(), account.balance.get_current(), touched_balance.clone());
                }
            }

            // check slots conflicts
            for (touched_slot_index, touched_slot) in &change.slots {
                if let Some(slot) = account.slots.get(touched_slot_index) {
                    if let Some(touched_slot) = touched_slot.take_original_ref() {
                        let slot_value = slot.get_current().value;
                        if touched_slot.value != slot_value {
                            conflicts.add_slot(address.clone(), touched_slot_index.clone(), slot_value, touched_slot.value.clone());
                        }
                    }
                }
            }
        }
    }

    conflicts.build()
}
