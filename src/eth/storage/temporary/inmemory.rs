//! In-memory storage implementations.

use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::RwLockReadGuard;
use std::sync::RwLockWriteGuard;

use nonempty::NonEmpty;

use crate::eth::executor::EvmInput;
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

/// Number of previous blocks to keep inmemory to detect conflicts between different blocks.
const MAX_BLOCKS: usize = 64;

#[derive(Debug)]
pub struct InMemoryTemporaryStorage {
    /// TODO: very inneficient, it is O(N), but it should be 0(1)
    pub states: RwLock<NonEmpty<InMemoryTemporaryStorageState>>,
}

impl InMemoryTemporaryStorage {
    pub fn new(block_number: BlockNumber) -> Self {
        Self {
            states: RwLock::new(NonEmpty::new(InMemoryTemporaryStorageState {
                block: PendingBlock::new_at_now(block_number),
                accounts: HashMap::default(),
            })),
        }
    }

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

#[derive(Debug)]
pub struct InMemoryTemporaryStorageState {
    /// Block that is being mined.
    pub block: PendingBlock,

    /// Last state of accounts and slots. Can be recreated from the executions inside the pending block.
    pub accounts: HashMap<Address, InMemoryTemporaryAccount, hash_hasher::HashBuildHasher>,
}

impl InMemoryTemporaryStorageState {
    pub fn new(block_number: BlockNumber) -> Self {
        Self {
            block: PendingBlock::new_at_now(block_number),
            accounts: HashMap::default(),
        }
    }

    pub fn reset(&mut self) {
        self.block = PendingBlock::new_at_now(1.into());
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

    // Uneeded clone here, return Cow
    fn read_pending_block_header(&self) -> PendingBlockHeader {
        let states = self.lock_read();
        states.head.block.header.clone()
    }

    // -------------------------------------------------------------------------
    // Block and executions
    // -------------------------------------------------------------------------

    fn save_pending_execution(&self, tx: TransactionExecution, check_conflicts: bool) -> Result<(), StratusError> {
        // check conflicts
        let mut states = self.lock_write();
        if let TransactionExecution::Local(tx) = &tx {
            tracing::info!("hereeeeee");

            let expected_input = EvmInput::from_eth_transaction(tx.input.clone(), states.head.block.header.clone());

            if expected_input != tx.evm_input {
                return Err(StratusError::TransactionEvmInputMismatch {
                    expected: expected_input,
                    actual: tx.evm_input.clone(),
                });
            }
        }

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

            // slots
            for slot in change.slots.values() {
                if let Some(slot) = slot.take_ref() {
                    account.slots.insert(slot.index, *slot);
                }
            }
        }

        // save execution
        states.head.block.push_transaction(tx);

        Ok(())
    }

    fn read_pending_executions(&self) -> Vec<TransactionExecution> {
        self.lock_read().head.block.transactions.iter().map(|(_, tx)| tx.clone()).collect()
    }

    /// TODO: we cannot allow more than one pending block. Where to put this check?
    fn finish_pending_block(&self) -> anyhow::Result<PendingBlock> {
        let mut states = self.lock_write();

        #[cfg(feature = "dev")]
        let mut finished_block = states.head.block.clone();
        #[cfg(not(feature = "dev"))]
        let finished_block = states.head.block.clone();

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
        states.insert(0, InMemoryTemporaryStorageState::new(finished_block.header.number.next_block_number()));

        Ok(finished_block)
    }

    fn read_pending_execution(&self, hash: Hash) -> anyhow::Result<Option<TransactionExecution>> {
        let states = self.lock_read();
        match states.head.block.transactions.get(&hash) {
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
    let slot = states
        .iter()
        .find_map(|state| state.accounts.get(&address).and_then(|account| account.slots.get(&index)));

    if let Some(&slot) = slot {
        tracing::trace!(%address, %index, %slot, "slot found in temporary");
        Some(slot)
    } else {
        tracing::trace!(%address, %index, "slot not found in temporary");
        None
    }
}

fn do_check_conflicts(states: &NonEmpty<InMemoryTemporaryStorageState>, execution: &EvmExecution) -> Option<ExecutionConflicts> {
    let mut conflicts = ExecutionConflictsBuilder::default();

    for (&address, change) in &execution.changes {
        // check account info conflicts
        if let Some(account) = do_read_account(states, address) {
            if let Some(expected) = change.nonce.take_original_ref() {
                let original = &account.nonce;
                if expected != original {
                    conflicts.add_nonce(address, *original, *expected);
                }
            }
            if let Some(expected) = change.balance.take_original_ref() {
                let original = &account.balance;
                if expected != original {
                    conflicts.add_balance(address, *original, *expected);
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
