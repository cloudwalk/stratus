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

use super::inmemory_account::InMemoryAccount;
use super::inmemory_account::InMemoryAccountPermanent;
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
use crate::eth::storage::PermanentStorage;
use crate::eth::storage::StorageError;

#[derive(Debug, Default)]
struct InMemoryPermanentStorageState {
    accounts: HashMap<Address, InMemoryAccountPermanent>,
    transactions: HashMap<Hash, TransactionMined>,
    blocks_by_number: IndexMap<BlockNumber, Arc<Block>>,
    blocks_by_hash: IndexMap<Hash, Arc<Block>>,
    logs: Vec<LogMined>,
}

#[derive(Debug)]
pub struct InMemoryStoragePermanent {
    state: RwLock<InMemoryPermanentStorageState>,
    block_number: AtomicU64,
}

impl InMemoryStoragePermanent {
    /// Locks inner state for reading.
    async fn lock_read(&self) -> RwLockReadGuard<'_, InMemoryPermanentStorageState> {
        self.state.read().await
    }

    /// Locks inner state for writing.
    async fn lock_write(&self) -> RwLockWriteGuard<'_, InMemoryPermanentStorageState> {
        self.state.write().await
    }

    /// Clears in-memory state.
    pub async fn clear(&self) {
        let mut state = self.lock_write().await;
        state.accounts.clear();
        state.transactions.clear();
        state.blocks_by_hash.clear();
        state.blocks_by_number.clear();
        state.logs.clear();
    }

    async fn save_account_changes(state: &mut InMemoryPermanentStorageState, block_number: BlockNumber, execution: Execution) {
        let is_success = execution.is_success();
        for changes in execution.changes {
            let account = state
                .accounts
                .entry(changes.address.clone())
                .or_insert_with(|| InMemoryAccountPermanent::new(changes.address));

            // account basic info
            if let Some(nonce) = changes.nonce.take_modified() {
                account.set_nonce(block_number, nonce);
            }
            if let Some(balance) = changes.balance.take_modified() {
                account.set_balance(block_number, balance);
            }

            // slots
            if is_success {
                if let Some(Some(bytecode)) = changes.bytecode.take_modified() {
                    account.set_bytecode(block_number, bytecode);
                }

                for (_, slot) in changes.slots {
                    if let Some(slot) = slot.take_modified() {
                        account.set_slot(block_number, slot);
                    }
                }
            }
        }
    }

    async fn check_conflicts(state: &InMemoryPermanentStorageState, execution: &Execution) -> Option<ExecutionConflicts> {
        let mut conflicts = ExecutionConflictsBuilder::default();

        for change in &execution.changes {
            let address = &change.address;

            if let Some(account) = state.accounts.get(address) {
                // check account info conflicts
                if let Some(touched_nonce) = change.nonce.take_original_ref() {
                    let nonce = account.get_current_nonce();
                    if touched_nonce != nonce {
                        conflicts.add_nonce(address.clone(), nonce.clone(), touched_nonce.clone());
                    }
                }
                if let Some(touched_balance) = change.balance.take_original_ref() {
                    let balance = account.get_current_balance();
                    if touched_balance != balance {
                        conflicts.add_balance(address.clone(), balance.clone(), touched_balance.clone());
                    }
                }

                // check slots conflicts
                for (touched_slot_index, touched_slot) in &change.slots {
                    if let Some(slot) = account.get_current_slot(touched_slot_index) {
                        if let Some(touched_slot) = touched_slot.take_original_ref() {
                            let slot_value = slot.value.clone();
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
}

impl Default for InMemoryStoragePermanent {
    fn default() -> Self {
        tracing::info!("starting inmemory storage");

        Self {
            state: RwLock::new(InMemoryPermanentStorageState::default()),
            block_number: Default::default(),
        }
    }
}

#[async_trait]
impl PermanentStorage for InMemoryStoragePermanent {
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

    async fn set_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        self.block_number.store(number.as_u64(), Ordering::SeqCst);
        Ok(())
    }

    // -------------------------------------------------------------------------
    // State operations
    // ------------------------------------------------------------------------

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

    async fn save_block(&self, block: Block) -> anyhow::Result<(), StorageError> {
        let mut state = self.lock_write().await;

        // keep track of current block if we need to rollback
        let current_block = self.read_current_block_number().await?;

        // save block
        tracing::debug!(number = %block.number(), "saving block");
        let block = Arc::new(block);
        state.blocks_by_number.insert(*block.number(), Arc::clone(&block));
        state.blocks_by_hash.insert(block.hash().clone(), Arc::clone(&block));

        // save transactions
        for transaction in block.transactions.clone() {
            tracing::debug!(hash = %transaction.input.hash, "saving transaction");

            // check conflicts after each transaction because a transaction can depend on the previous from the same block
            if let Some(conflicts) = Self::check_conflicts(&state, &transaction.execution).await {
                // release lock and rollback to previous block
                drop(state);
                PermanentStorage::reset_at(self, current_block).await?;

                // inform error
                return Err(StorageError::Conflict(conflicts));
            }

            // save transaction
            state.transactions.insert(transaction.input.hash.clone(), transaction.clone());

            // save logs
            if transaction.is_success() {
                for log in transaction.logs {
                    state.logs.push(log);
                }
            }

            // save execution changes
            Self::save_account_changes(&mut state, *block.number(), transaction.execution).await;
        }
        Ok(())
    }

    async fn save_accounts(&self, accounts: Vec<Account>) -> anyhow::Result<()> {
        tracing::debug!(?accounts, "saving initial accounts");

        let mut state = self.lock_write().await;
        for account in accounts {
            state.accounts.insert(
                account.address.clone(),
                InMemoryAccountPermanent::new_with_balance(account.address, account.balance),
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
}
