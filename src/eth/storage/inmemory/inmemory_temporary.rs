//! In-memory storage implementations.

use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::RwLock;
use tokio::sync::RwLockReadGuard;
use tokio::sync::RwLockWriteGuard;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Execution;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::storage::TemporaryStorage;

#[derive(Debug, Default)]
pub struct InMemoryTemporaryStorage {
    state: RwLock<InMemoryTemporaryStorageState>,
}

#[derive(Debug, Default)]
struct InMemoryTemporaryStorageState {
    accounts: HashMap<Address, InMemoryTemporaryAccount>,
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
    async fn maybe_read_account(&self, address: &Address, _point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>> {
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

    async fn maybe_read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Slot>> {
        tracing::debug!(%address, %slot_index, ?point_in_time, "reading slot");

        let state = self.lock_read().await;
        let Some(account) = state.accounts.get(address) else {
            tracing::trace!(%address, "account not found");
            return Ok(Default::default());
        };

        match account.slots.get(slot_index) {
            Some(slot) => {
                tracing::trace!(%address, %slot_index, ?point_in_time, %slot, "slot found");
                Ok(Some(slot.clone()))
            }

            None => {
                tracing::trace!(%address, %slot_index, ?point_in_time, "slot not found");
                Ok(None)
            }
        }
    }

    async fn save_account_changes(&self, execution: Execution) -> anyhow::Result<()> {
        let mut state = self.lock_write().await;
        for changes in execution.changes {
            let account = state
                .accounts
                .entry(changes.address.clone())
                .or_insert_with(|| InMemoryTemporaryAccount::new(changes.address));

            // account basic info
            if let Some(nonce) = changes.nonce.take() {
                account.info.nonce = nonce;
            }
            if let Some(balance) = changes.balance.take() {
                account.info.balance = balance;
            }
            if let Some(Some(bytecode)) = changes.bytecode.take() {
                account.info.bytecode = Some(bytecode);
            }

            // slots
            for (_, slot) in changes.slots {
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
