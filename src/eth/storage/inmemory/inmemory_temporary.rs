//! In-memory storage implementations.

use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::RwLock;
use tokio::sync::RwLockReadGuard;
use tokio::sync::RwLockWriteGuard;

use super::inmemory_account::InMemoryAccountTemporary;
use super::inmemory_state::StorageState;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Execution;
use crate::eth::primitives::ExecutionConflicts;
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

impl StorageState<InMemoryAccountTemporary> for InMemoryTemporaryStorageState {
    fn get_accounts(&self) -> &HashMap<Address, InMemoryAccountTemporary> {
        &self.accounts
    }

    fn get_accounts_mut(&mut self) -> &mut HashMap<Address, InMemoryAccountTemporary> {
        &mut self.accounts
    }
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
    // -------------------------------------------------------------------------
    // State operations
    // ------------------------------------------------------------------------

    async fn check_conflicts(&self, execution: &Execution) -> anyhow::Result<Option<ExecutionConflicts>> {
        let state_lock = self.lock_read().await;
        Ok(state_lock.check_conflicts(execution))
    }

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
        let mut state_lock = self.lock_write().await;
        state_lock.save_account_changes(number, execution);
        Ok(())
    }

    async fn reset(&self) -> anyhow::Result<()> {
        let mut state = self.lock_write().await;
        state.accounts.clear();
        Ok(())
    }
}
