//! In-memory storage implementations.

use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::RwLock;
use tokio::sync::RwLockReadGuard;
use tokio::sync::RwLockWriteGuard;

use super::inmemory_account::InMemoryAccount;
use super::inmemory_account::InMemoryAccountTemporary;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Execution;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::storage::TemporaryStorage;

#[derive(Debug, Default)]
pub struct InMemoryStorageTemporary {
    state: RwLock<InMemoryTemporaryStorageState>,
}

#[derive(Debug, Default)]
struct InMemoryTemporaryStorageState {
    accounts: HashMap<Address, InMemoryAccountTemporary>,
}

impl InMemoryStorageTemporary {
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
impl TemporaryStorage for InMemoryStorageTemporary {
    async fn maybe_read_account(&self, address: &Address, _point_in_time: &StoragePointInTime) -> anyhow::Result<Option<Account>> {
        tracing::debug!(%address, "reading account");

        let state = self.lock_read().await;

        match state.accounts.get(address) {
            Some(account) => {
                let account = Account {
                    address: address.clone(),
                    balance: account.balance.clone(),
                    nonce: account.nonce.clone(),
                    bytecode: account.bytecode.clone(),
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

    async fn save_account_changes(&self, number: BlockNumber, execution: Execution) -> anyhow::Result<()> {
        let mut state = self.lock_write().await;
        let is_success = execution.is_success();
        for changes in execution.changes {
            let account = state
                .accounts
                .entry(changes.address.clone())
                .or_insert_with(|| InMemoryAccountTemporary::new(changes.address));

            // account basic info
            if let Some(nonce) = changes.nonce.take() {
                account.set_nonce(number, nonce);
            }
            if let Some(balance) = changes.balance.take() {
                account.set_balance(number, balance);
            }
            if let Some(Some(bytecode)) = changes.bytecode.take() {
                account.set_bytecode(number, bytecode);
            }

            // slots
            if is_success {
                for (_, slot) in changes.slots {
                    if let Some(slot) = slot.take_modified() {
                        account.set_slot(number, slot);
                    }
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
