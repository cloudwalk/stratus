//! In-memory storage implementations.

use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::RwLock;
use tokio::sync::RwLockReadGuard;
use tokio::sync::RwLockWriteGuard;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::storage::TemporaryStorage;

#[derive(Debug)]
pub struct InMemoryTemporaryStorage {
    state: RwLock<InMemoryTemporaryStorageState>,
}

#[derive(Debug, Default)]
struct InMemoryTemporaryStorageState {
    accounts: HashMap<Address, InMemoryTemporaryAccount>,
    active_block_number: BlockNumber,
}

impl Default for InMemoryTemporaryStorage {
    fn default() -> Self {
        tracing::info!("starting inmemory temporary storage");
        Self { state: Default::default() }
    }
}

impl InMemoryTemporaryStorage {
    /// Locks inner state for reading.
    async fn lock_read(&self) -> RwLockReadGuard<'_, InMemoryTemporaryStorageState> {
        self.state.read().await
    }

    /// Locks inner state for writing.
    async fn lock_write(&self) -> RwLockWriteGuard<'_, InMemoryTemporaryStorageState> {
        self.state.write().await
    }
}

#[async_trait]
impl TemporaryStorage for InMemoryTemporaryStorage {
    async fn set_active_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        let mut state = self.lock_write().await;
        state.active_block_number = number;
        Ok(())
    }

    async fn read_active_block_number(&self) -> anyhow::Result<BlockNumber> {
        let state = self.lock_read().await;
        Ok(state.active_block_number)
    }

    async fn maybe_read_account(&self, address: &Address) -> anyhow::Result<Option<Account>> {
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

    async fn maybe_read_slot(&self, address: &Address, slot_index: &SlotIndex) -> anyhow::Result<Option<Slot>> {
        tracing::debug!(%address, %slot_index, "reading slot");

        let state = self.lock_read().await;
        let Some(account) = state.accounts.get(address) else {
            tracing::trace!(%address, "account not found");
            return Ok(Default::default());
        };

        match account.slots.get(slot_index) {
            Some(slot) => {
                tracing::trace!(%address, %slot_index, %slot, "slot found");
                Ok(Some(slot.clone()))
            }

            None => {
                tracing::trace!(%address, %slot_index, "slot not found");
                Ok(None)
            }
        }
    }

    async fn save_account_changes(&self, changes: Vec<ExecutionAccountChanges>) -> anyhow::Result<()> {
        let mut state = self.lock_write().await;
        for change in changes {
            let account = state
                .accounts
                .entry(change.address.clone())
                .or_insert_with(|| InMemoryTemporaryAccount::new(change.address));

            // account basic info
            if let Some(nonce) = change.nonce.take() {
                account.info.nonce = nonce;
            }
            if let Some(balance) = change.balance.take() {
                account.info.balance = balance;
            }
            if let Some(Some(bytecode)) = change.bytecode.take() {
                account.info.bytecode = Some(bytecode);
            }

            // slots
            for (_, slot) in change.slots {
                if let Some(slot) = slot.take() {
                    account.slots.insert(slot.index.clone(), slot);
                }
            }
        }
        Ok(())
    }

    async fn reset(&self) -> anyhow::Result<()> {
        let mut state = self.lock_write().await;
        state.accounts.clear();
        Ok(())
    }
}

#[derive(Debug)]
struct InMemoryTemporaryAccount {
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
