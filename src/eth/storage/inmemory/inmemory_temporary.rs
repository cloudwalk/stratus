//! In-memory storage implementations.

use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::RwLock;
use tokio::sync::RwLockReadGuard;
use tokio::sync::RwLockWriteGuard;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::TransactionExecution;
use crate::eth::storage::TemporaryStorage;

#[derive(Debug, Default)]
pub struct InMemoryTemporaryStorageState {
    pub transaction_executions: Vec<TransactionExecution>,
    pub accounts: HashMap<Address, InMemoryTemporaryAccount>,
    pub active_block_number: Option<BlockNumber>,
}

impl InMemoryTemporaryStorageState {
    pub fn reset(&mut self) {
        self.transaction_executions.clear();
        self.accounts.clear();
        self.active_block_number = None;
    }
}

#[derive(Debug)]
pub struct InMemoryTemporaryStorage {
    pub state: RwLock<InMemoryTemporaryStorageState>,
}

impl InMemoryTemporaryStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for InMemoryTemporaryStorage {
    fn default() -> Self {
        tracing::info!("starting inmemory temporary storage");
        Self { state: Default::default() }
    }
}

impl InMemoryTemporaryStorage {
    /// Locks inner state for reading.
    pub async fn lock_read(&self) -> RwLockReadGuard<'_, InMemoryTemporaryStorageState> {
        self.state.read().await
    }

    /// Locks inner state for writing.
    pub async fn lock_write(&self) -> RwLockWriteGuard<'_, InMemoryTemporaryStorageState> {
        self.state.write().await
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

    /// TODO: temporary stuff while block-per-second is being implemented.
    async fn read_executions(&self) -> Vec<TransactionExecution> {
        let state = self.lock_read().await;
        state.transaction_executions.clone()
    }

    async fn save_execution(&self, transaction_execution: TransactionExecution) -> anyhow::Result<()> {
        let mut state = self.lock_write().await;

        // save account changes
        let changes = transaction_execution.execution.changes_to_persist();
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

        // save executions
        state.transaction_executions.push(transaction_execution);

        Ok(())
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
