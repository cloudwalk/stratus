//! In-memory storage implementations.

use std::collections::HashMap;

use anyhow::Context;
use async_trait::async_trait;
use tokio::sync::RwLock;
use tokio::sync::RwLockReadGuard;
use tokio::sync::RwLockWriteGuard;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::ExecutionConflicts;
use crate::eth::primitives::ExecutionConflictsBuilder;
use crate::eth::primitives::ExternalBlock;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::TransactionExecution;
use crate::eth::storage::temporary_storage::TemporaryStorageExecutionOps;
use crate::eth::storage::StorageError;
use crate::eth::storage::TemporaryStorage;

#[derive(Debug)]
pub struct InMemoryTemporaryStorage {
    pub state: RwLock<InMemoryTemporaryStorageState>,
}

impl InMemoryTemporaryStorage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Locks inner state for reading.
    pub async fn lock_read(&self) -> RwLockReadGuard<'_, InMemoryTemporaryStorageState> {
        self.state.read().await
    }

    /// Locks inner state for writing.
    pub async fn lock_write(&self) -> RwLockWriteGuard<'_, InMemoryTemporaryStorageState> {
        self.state.write().await
    }
}

impl Default for InMemoryTemporaryStorage {
    fn default() -> Self {
        tracing::info!("creating inmemory temporary storage");
        Self { state: Default::default() }
    }
}

// -----------------------------------------------------------------------------
// Inner State
// -----------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct InMemoryTemporaryStorageState {
    /// External block being re-executed.
    pub external_block: Option<ExternalBlock>,

    /// Pending transactions executions during block execution.
    pub tx_executions: Vec<TransactionExecution>,

    /// Pending accounts modified during block execution.
    pub accounts: HashMap<Address, InMemoryTemporaryAccount>,

    /// Pending slots modified during block execution.
    pub active_block_number: Option<BlockNumber>,
}

impl InMemoryTemporaryStorageState {
    pub fn reset(&mut self) {
        self.external_block = None;
        self.tx_executions.clear();
        self.accounts.clear();
        self.active_block_number = None;
    }

    /// Checks if a new execution conflicts with the current state.
    fn check_conflicts(&self, execution: &EvmExecution) -> Option<ExecutionConflicts> {
        let mut conflicts = ExecutionConflictsBuilder::default();

        for (address, change) in &execution.changes {
            let account = match self.accounts.get(address) {
                Some(account) => account,
                None => continue,
            };
            // check account info conflicts
            if let Some(expected) = change.nonce.take_original_ref() {
                let original = &account.info.nonce;
                if expected != original {
                    conflicts.add_nonce(*address, *original, *expected);
                }
            }
            if let Some(expected) = change.balance.take_original_ref() {
                let original = &account.info.balance;
                if expected != original {
                    conflicts.add_balance(*address, *original, *expected);
                }
            }

            // check slots conflicts
            for (slot_index, slot_change) in &change.slots {
                let original = match account.slots.get(slot_index) {
                    Some(slot) => slot,
                    None => continue,
                };
                if let Some(expected) = slot_change.take_original_ref() {
                    if expected.value != original.value {
                        conflicts.add_slot(*address, *slot_index, original.value, expected.value);
                    }
                }
            }
        }

        conflicts.build()
    }
}

#[async_trait]
impl TemporaryStorageExecutionOps for InMemoryTemporaryStorage {
    async fn set_external_block(&self, block: ExternalBlock) -> anyhow::Result<()> {
        let mut state = self.lock_write().await;
        state.external_block = Some(block);
        Ok(())
    }

    async fn read_external_block(&self) -> anyhow::Result<Option<ExternalBlock>> {
        let state = self.lock_read().await;
        Ok(state.external_block.clone())
    }

    async fn save_execution(&self, tx: TransactionExecution) -> anyhow::Result<()> {
        let mut state = self.lock_write().await;
        tracing::debug!(hash = %tx.hash(), tx_executions_len = %state.tx_executions.len(), "saving execution");

        // check conflicts
        if let Some(conflicts) = state.check_conflicts(&tx.execution) {
            return Err(StorageError::Conflict(conflicts)).context("execution conflicts with current state");
        }

        // save account changes
        let changes = tx.execution.changes_to_persist();
        for change in changes {
            let account = state
                .accounts
                .entry(change.address)
                .or_insert_with(|| InMemoryTemporaryAccount::new(change.address));

            // account basic info
            if let Some(nonce) = change.nonce.take() {
                account.info.nonce = nonce;
            }
            if let Some(balance) = change.balance.take() {
                account.info.balance = balance;
            }

            // bytecode (todo: where is code_hash?)
            if let Some(Some(bytecode)) = change.bytecode.take() {
                account.info.bytecode = Some(bytecode);
            }
            if let Some(indexes) = change.static_slot_indexes.take() {
                account.info.static_slot_indexes = indexes;
            }
            if let Some(indexes) = change.mapping_slot_indexes.take() {
                account.info.mapping_slot_indexes = indexes;
            }

            // slots
            for (_, slot) in change.slots {
                if let Some(slot) = slot.take() {
                    account.slots.insert(slot.index, slot);
                }
            }
        }

        // save execution
        state.tx_executions.push(tx);

        Ok(())
    }

    async fn read_executions(&self) -> anyhow::Result<Vec<TransactionExecution>> {
        tracing::debug!("reading executions");
        let state = self.lock_read().await;
        Ok(state.tx_executions.clone())
    }

    async fn remove_executions_before(&self, index: usize) -> anyhow::Result<()> {
        if index == 0 {
            return Ok(());
        }

        let mut state = self.lock_write().await;
        tracing::debug!(tx_executions_len = %state.tx_executions.len(), index = %index, "removing executions");
        let _ = state.tx_executions.drain(..index - 1);

        Ok(())
    }
}

#[async_trait]
impl TemporaryStorage for InMemoryTemporaryStorage {
    async fn set_active_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        let mut state = self.lock_write().await;
        state.active_block_number = Some(number);
        Ok(())
    }

    async fn read_active_block_number(&self) -> anyhow::Result<Option<BlockNumber>> {
        let state = self.lock_read().await;
        Ok(state.active_block_number)
    }

    async fn read_account(&self, address: &Address) -> anyhow::Result<Option<Account>> {
        tracing::debug!(%address, "reading account");

        let state = self.lock_read().await;
        match state.accounts.get(address) {
            Some(account) => {
                let info = account.info.clone();
                let account = Account {
                    address: info.address,
                    balance: info.balance,
                    nonce: info.nonce,
                    bytecode: info.bytecode,
                    code_hash: info.code_hash,
                    static_slot_indexes: info.static_slot_indexes,
                    mapping_slot_indexes: info.mapping_slot_indexes,
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

    async fn read_slot(&self, address: &Address, index: &SlotIndex) -> anyhow::Result<Option<Slot>> {
        tracing::debug!(%address, %index, "reading slot");

        let state = self.lock_read().await;
        let Some(account) = state.accounts.get(address) else {
            tracing::trace!(%address, "account not found");
            return Ok(Default::default());
        };

        match account.slots.get(index) {
            Some(slot) => {
                tracing::trace!(%address, %index, %slot, "slot found");
                Ok(Some(*slot))
            }

            None => {
                tracing::trace!(%address, %index, "slot not found");
                Ok(None)
            }
        }
    }

    async fn flush(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn reset(&self) -> anyhow::Result<()> {
        let mut state = self.lock_write().await;
        state.reset();
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct InMemoryTemporaryAccount {
    pub info: Account,
    pub slots: HashMap<SlotIndex, Slot>,
}

impl InMemoryTemporaryAccount {
    /// Creates a new temporary account.
    fn new(address: Address) -> Self {
        Self {
            info: Account::new_empty(address),
            slots: Default::default(),
        }
    }
}
