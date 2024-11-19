//! In-memory storage implementations.

use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::RwLockReadGuard;
use std::sync::RwLockWriteGuard;

use nonempty::NonEmpty;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::ExecutionConflicts;
use crate::eth::primitives::ExecutionConflictsBuilder;
use crate::eth::primitives::Hash;
use crate::eth::primitives::PendingBlock;
use crate::eth::primitives::PendingBlockHeader;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StratusError;
use crate::eth::primitives::TransactionExecution;
#[cfg(feature = "dev")]
use crate::eth::primitives::UnixTime;
#[cfg(feature = "dev")]
use crate::eth::primitives::UnixTimeNow;
use crate::eth::storage::TemporaryStorage;
use crate::log_and_err;

/// Number of previous blocks to keep inmemory to detect conflicts between different blocks.
const MAX_BLOCKS: usize = 64;

#[derive(Debug)]
pub struct InMemoryTemporaryStorage {
    /// TODO: very inneficient, it is O(N), but it should be 0(1)
    pub states: RwLock<NonEmpty<InMemoryTemporaryStorageState>>,
}

impl Default for InMemoryTemporaryStorage {
    fn default() -> Self {
        tracing::info!("creating inmemory temporary storage");
        Self {
            states: RwLock::new(NonEmpty::new(InMemoryTemporaryStorageState::default())),
        }
    }
}

impl InMemoryTemporaryStorage {
    /// Locks inner state for reading.
    pub fn lock_read(&self) -> RwLockReadGuard<'_, NonEmpty<InMemoryTemporaryStorageState>> {
        self.states.read().unwrap()
    }

    /// Locks inner state for writing.
    pub fn lock_write(&self) -> RwLockWriteGuard<'_, NonEmpty<InMemoryTemporaryStorageState>> {
        self.states.write().unwrap()
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
    pub accounts: HashMap<Address, InMemoryTemporaryAccount, hash_hasher::HashBuildHasher>,
}

impl InMemoryTemporaryStorageState {
    /// Validates there is a pending block being mined and returns a reference to it.
    fn require_pending_block(&self) -> anyhow::Result<&PendingBlock> {
        match &self.block {
            Some(block) => Ok(block),
            None => log_and_err!("no pending block being mined"), // try calling set_pending_block_number_as_next_if_not_set or any other method to create a new block on temp storage
        }
    }

    /// Validates there is a pending block being mined and returns a mutable reference to it.
    fn require_pending_block_mut(&mut self) -> anyhow::Result<&mut PendingBlock> {
        match &mut self.block {
            Some(block) => Ok(block),
            None => log_and_err!("no pending block being mined"), // try calling set_pending_block_number_as_next_if_not_set or any other method to create a new block on temp storage
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
    pub slots: HashMap<SlotIndex, Slot, hash_hasher::HashBuildHasher>,
}

impl InMemoryTemporaryAccount {
    /// Creates a new temporary account.
    fn new(address: Address) -> Self {
        Self {
            info: Account::new_empty(address),
            slots: HashMap::default(),
        }
    }
}

impl TemporaryStorage for InMemoryTemporaryStorage {
    // -------------------------------------------------------------------------
    // Block number
    // -------------------------------------------------------------------------

    fn set_pending_block_number(&self, number: BlockNumber) -> anyhow::Result<()> {
        let mut states = self.lock_write();
        match states.head.block.as_mut() {
            Some(block) => block.header.number = number,
            None => {
                states.head.block = Some(PendingBlock::new_at_now(number));
            }
        }
        Ok(())
    }

    fn read_pending_block_header(&self) -> anyhow::Result<Option<PendingBlockHeader>> {
        let states = self.lock_read();
        match &states.head.block {
            Some(block) => Ok(Some(block.header.clone())),
            None => Ok(None),
        }
    }

    // -------------------------------------------------------------------------
    // Block and executions
    // -------------------------------------------------------------------------

    fn save_pending_execution(&self, tx: TransactionExecution, check_conflicts: bool) -> Result<(), StratusError> {
        // check conflicts
        let mut states = self.lock_write();
        if check_conflicts {
            if let Some(conflicts) = do_check_conflicts(&states, tx.execution()) {
                return Err(StratusError::TransactionConflict(conflicts.into()));
            }
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
            if let Some(&nonce) = change.nonce.take_ref() {
                account.info.nonce = nonce;
            }
            if let Some(balance) = change.balance.take_ref() {
                account.info.balance = *balance;
            }

            // bytecode (todo: where is code_hash?)
            if let Some(Some(bytecode)) = change.bytecode.take_ref() {
                account.info.bytecode = Some(bytecode.clone());
            }

            // slots
            for slot in change.slots.values() {
                if let Some(&slot) = slot.take_ref() {
                    account.slots.insert(slot.index, slot);
                }
            }
        }

        // save execution
        states.head.require_pending_block_mut()?.push_transaction(tx);

        Ok(())
    }

    fn read_pending_executions(&self) -> Vec<TransactionExecution> {
        self.lock_read()
            .head
            .block
            .as_ref()
            .map(|pending_block| pending_block.transactions.iter().map(|(_, tx)| tx.clone()).collect())
            .unwrap_or_default()
    }

    /// TODO: we cannot allow more than one pending block. Where to put this check?
    fn finish_pending_block(&self) -> anyhow::Result<PendingBlock> {
        let mut states = self.lock_write();

        #[cfg(feature = "dev")]
        let mut finished_block = states.head.require_pending_block()?.clone();
        #[cfg(not(feature = "dev"))]
        let finished_block = states.head.require_pending_block()?.clone();

        #[cfg(feature = "dev")]
        {
            // Update block timestamp only if evm_setNextBlockTimestamp was called,
            // otherwise keep the original timestamp from pending block creation
            if UnixTime::evm_set_next_block_timestamp_was_called() {
                finished_block.header.timestamp = UnixTimeNow::default();
            }
        }

        // remove last state if reached limit
        if states.len() + 1 >= MAX_BLOCKS {
            let _ = states.pop();
        }

        // create new state
        states.insert(0, InMemoryTemporaryStorageState::default());
        states.head.block = Some(PendingBlock::new_at_now(finished_block.header.number.next_block_number()));

        Ok(finished_block)
    }

    fn read_pending_execution(&self, hash: Hash) -> anyhow::Result<Option<TransactionExecution>> {
        let states = self.lock_read();
        let Some(ref pending_block) = states.head.block else { return Ok(None) };
        match pending_block.transactions.get(&hash) {
            Some(tx) => Ok(Some(tx.clone())),
            None => Ok(None),
        }
    }

    // -------------------------------------------------------------------------
    // Accounts and Slots
    // -------------------------------------------------------------------------

    fn read_account(&self, address: Address) -> anyhow::Result<Option<Account>> {
        let states = self.lock_read();
        Ok(do_read_account(&states, address))
    }

    fn read_slot(&self, address: Address, index: SlotIndex) -> anyhow::Result<Option<Slot>> {
        let states = self.lock_read();
        Ok(do_read_slot(&states, address, index))
    }

    // -------------------------------------------------------------------------
    // Global state
    // -------------------------------------------------------------------------
    fn reset(&self) -> anyhow::Result<()> {
        let mut state = self.lock_write();
        state.tail.clear();
        state.head.reset();
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Implementations without lock
// -----------------------------------------------------------------------------
fn do_read_account(states: &NonEmpty<InMemoryTemporaryStorageState>, address: Address) -> Option<Account> {
    // search all
    for state in states.iter() {
        let Some(account) = state.accounts.get(&address) else { continue };

        let info = account.info.clone();
        let account = Account {
            address: info.address,
            balance: info.balance,
            nonce: info.nonce,
            bytecode: info.bytecode,
            code_hash: info.code_hash,
        };

        tracing::trace!(%address, ?account, "account found");
        return Some(account);
    }

    // not found
    tracing::trace!(%address, "account not found");
    None
}

fn do_read_slot(states: &NonEmpty<InMemoryTemporaryStorageState>, address: Address, index: SlotIndex) -> Option<Slot> {
    // search all
    for state in states.iter() {
        let Some(account) = state.accounts.get(&address) else { continue };
        let Some(&slot) = account.slots.get(&index) else { continue };

        tracing::trace!(%address, %index, %slot, "slot found in temporary");
        return Some(slot);
    }

    // not found
    tracing::trace!(%address, %index, "slot not found in temporary");
    None
}

fn do_check_conflicts(states: &NonEmpty<InMemoryTemporaryStorageState>, execution: &EvmExecution) -> Option<ExecutionConflicts> {
    let mut conflicts = ExecutionConflictsBuilder::default();

    for (&address, change) in &execution.changes {
        // check account info conflicts
        if let Some(account) = do_read_account(states, address) {
            if let Some(&expected) = change.nonce.take_original_ref() {
                let original = account.nonce;
                if expected != original {
                    conflicts.add_nonce(address, original, expected);
                }
            }
            if let Some(&expected) = change.balance.take_original_ref() {
                let original = account.balance;
                if expected != original {
                    conflicts.add_balance(address, original, expected);
                }
            }
        }

        // check slots conflicts
        for (&slot_index, slot_change) in &change.slots {
            if let Some(expected) = slot_change.take_original_ref() {
                let Some(original) = do_read_slot(states, address, slot_index) else {
                    continue;
                };
                if expected.value != original.value {
                    conflicts.add_slot(address, slot_index, original.value, expected.value);
                }
            }
        }
    }

    conflicts.build()
}
