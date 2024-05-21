//! In-memory storage implementations.

use std::collections::HashMap;

use anyhow::Context;
use anyhow::Ok;
use async_trait::async_trait;
use nonempty::NonEmpty;
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
use crate::eth::primitives::PendingBlock;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::TransactionExecution;
use crate::eth::storage::StorageError;
use crate::eth::storage::TemporaryStorage;
use crate::log_and_err;

/// Number of previous blocks to keep inmemory to detect conflicts between different blocks.
const MAX_BLOCKS: usize = 64;

#[derive(Debug)]
pub struct InMemoryTemporaryStorage {
    /// TODO: very inneficient, it is O(N), but it should be 0(1)
    pub states: RwLock<NonEmpty<InMemoryTemporaryStorageState>>,
}

impl InMemoryTemporaryStorage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Locks inner state for reading.
    pub async fn lock_read(&self) -> RwLockReadGuard<'_, NonEmpty<InMemoryTemporaryStorageState>> {
        self.states.read().await
    }

    /// Locks inner state for writing.
    pub async fn lock_write(&self) -> RwLockWriteGuard<'_, NonEmpty<InMemoryTemporaryStorageState>> {
        self.states.write().await
    }
}

impl Default for InMemoryTemporaryStorage {
    fn default() -> Self {
        tracing::info!("starting inmemory temporary storage");
        Self {
            states: RwLock::new(NonEmpty::new(InMemoryTemporaryStorageState::default())),
        }
    }
}

// -----------------------------------------------------------------------------
// Inner State
// -----------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct InMemoryTemporaryStorageState {
    /// Block that is being mined.
    pub block: Option<PendingBlock>,

    /// Last state of accounts and slots. Can be recreated from the executions inside the pending block.
    pub accounts: HashMap<Address, InMemoryTemporaryAccount>,
}

impl InMemoryTemporaryStorageState {
    /// Validates there is an active pending block being mined and returns a reference to it.
    fn require_active_block(&mut self) -> anyhow::Result<&PendingBlock> {
        match &self.block {
            Some(block) => Ok(block),
            None => log_and_err!("no pending block being mined"),
        }
    }

    /// Validates there is an active pending block being mined and returns a mutable reference to it.
    fn require_active_block_mut(&mut self) -> anyhow::Result<&mut PendingBlock> {
        match &mut self.block {
            Some(block) => Ok(block),
            None => log_and_err!("no pending block being mined"),
        }
    }
}

impl InMemoryTemporaryStorageState {
    pub fn reset(&mut self) {
        self.block = None;
        self.accounts.clear();
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

#[async_trait]
impl TemporaryStorage for InMemoryTemporaryStorage {
    // -------------------------------------------------------------------------
    // Accounts and Slots
    // -------------------------------------------------------------------------

    async fn read_account(&self, address: &Address) -> anyhow::Result<Option<Account>> {
        tracing::debug!(%address, "reading account");
        let states = self.lock_read().await;
        Ok(read_account(&states, address))
    }

    async fn read_slot(&self, address: &Address, index: &SlotIndex) -> anyhow::Result<Option<Slot>> {
        tracing::debug!(%address, %index, "reading slot in temporary");
        let states = self.lock_read().await;
        Ok(read_slot(&states, address, index))
    }

    // -------------------------------------------------------------------------
    // Block number
    // -------------------------------------------------------------------------

    async fn set_active_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        tracing::debug!(%number, "setting active block number");

        let mut states = self.lock_write().await;
        match states.head.block.as_mut() {
            Some(block) => block.number = number,
            None => {
                states.head.block = Some(PendingBlock::new(number));
            }
        }
        Ok(())
    }

    async fn read_active_block_number(&self) -> anyhow::Result<Option<BlockNumber>> {
        tracing::debug!("reading active block number");

        let states = self.lock_read().await;
        match &states.head.block {
            Some(block) => Ok(Some(block.number)),
            None => Ok(None),
        }
    }

    // -------------------------------------------------------------------------
    // External block
    // -------------------------------------------------------------------------

    async fn set_active_external_block(&self, block: ExternalBlock) -> anyhow::Result<()> {
        tracing::debug!(number = %block.number(), "setting re-executed external block");

        let mut states = self.lock_write().await;
        states.head.require_active_block_mut()?.external_block = Some(block);
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Executions
    // -------------------------------------------------------------------------

    async fn save_execution(&self, tx: TransactionExecution) -> anyhow::Result<()> {
        tracing::debug!(hash = %tx.hash(), "saving execution");

        // check conflicts
        let mut states = self.lock_write().await;
        if let Some(conflicts) = check_conflicts(&states, tx.execution()) {
            return Err(StorageError::Conflict(conflicts)).context("execution conflicts with current state");
        }

        // save account changes
        let changes = tx.execution().changes.values();
        for change in changes {
            let account = states
                .head
                .accounts
                .entry(change.address)
                .or_insert_with(|| InMemoryTemporaryAccount::new(change.address));

            // account basic info
            if let Some(nonce) = change.nonce.take_ref() {
                account.info.nonce = *nonce;
            }
            if let Some(balance) = change.balance.take_ref() {
                account.info.balance = *balance;
            }

            // bytecode (todo: where is code_hash?)
            if let Some(Some(bytecode)) = change.bytecode.take_ref() {
                account.info.bytecode = Some(bytecode.clone());
            }
            if let Some(indexes) = change.static_slot_indexes.take_ref() {
                account.info.static_slot_indexes = indexes.clone();
            }
            if let Some(indexes) = change.mapping_slot_indexes.take_ref() {
                account.info.mapping_slot_indexes = indexes.clone();
            }

            // slots
            for slot in change.slots.values() {
                if let Some(slot) = slot.take_ref() {
                    account.slots.insert(slot.index, *slot);
                }
            }
        }

        // save execution
        states.head.require_active_block_mut()?.tx_executions.push(tx);

        Ok(())
    }

    // -------------------------------------------------------------------------
    // General state
    // -------------------------------------------------------------------------

    async fn check_conflicts(&self, execution: &EvmExecution) -> anyhow::Result<Option<ExecutionConflicts>> {
        tracing::debug!("checking conflicts");
        let states = self.lock_read().await;
        Ok(check_conflicts(&states, execution))
    }

    /// TODO: we cannot allow more than one pending block. Where to put this check?
    async fn finish_block(&self) -> anyhow::Result<PendingBlock> {
        tracing::debug!("finishing active block");

        let mut states = self.lock_write().await;
        let finished_block = states.head.require_active_block()?.clone();

        // remove last state if reached limit
        if states.len() + 1 >= MAX_BLOCKS {
            let _ = states.pop();
        }

        // create new state
        states.insert(0, InMemoryTemporaryStorageState::default());
        states.head.block = Some(PendingBlock::new(finished_block.number.next()));

        Ok(finished_block)
    }

    async fn reset(&self) -> anyhow::Result<()> {
        tracing::debug!("reseting temporary storage");

        let mut state = self.lock_write().await;
        state.tail.clear();
        state.head.reset();
        Ok(())
    }

    async fn flush(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Implementations without lock
// -----------------------------------------------------------------------------
fn read_account(states: &NonEmpty<InMemoryTemporaryStorageState>, address: &Address) -> Option<Account> {
    tracing::debug!(%address, "reading account");

    // search all
    for state in states.iter() {
        let Some(account) = state.accounts.get(address) else { continue };

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
        return Some(account);
    }

    // not found
    tracing::trace!(%address, "account not found");
    None
}

fn read_slot(states: &NonEmpty<InMemoryTemporaryStorageState>, address: &Address, index: &SlotIndex) -> Option<Slot> {
    tracing::debug!(%address, %index, "reading slot in temporary");

    // search all
    for state in states.iter() {
        let Some(account) = state.accounts.get(address) else { continue };
        let Some(slot) = account.slots.get(index) else { continue };

        tracing::trace!(%address, %index, %slot, "slot found in temporary");
        return Some(*slot);
    }

    // not found
    tracing::trace!(%address, %index, "slot not found in temporary");
    None
}

fn check_conflicts(states: &NonEmpty<InMemoryTemporaryStorageState>, execution: &EvmExecution) -> Option<ExecutionConflicts> {
    tracing::debug!("checking conflicts");
    let mut conflicts = ExecutionConflictsBuilder::default();

    for (address, change) in &execution.changes {
        // check account info conflicts
        if let Some(account) = read_account(states, address) {
            if let Some(expected) = change.nonce.take_original_ref() {
                let original = &account.nonce;
                if expected != original {
                    conflicts.add_nonce(*address, *original, *expected);
                }
            }
            if let Some(expected) = change.balance.take_original_ref() {
                let original = &account.balance;
                if expected != original {
                    conflicts.add_balance(*address, *original, *expected);
                }
            }
        }

        // check slots conflicts
        for (slot_index, slot_change) in &change.slots {
            if let Some(expected) = slot_change.take_original_ref() {
                let Some(original) = read_slot(states, address, slot_index) else {
                    continue;
                };
                if expected.value != original.value {
                    conflicts.add_slot(*address, *slot_index, original.value, expected.value);
                }
            }
        }
    }

    conflicts.build()
}
