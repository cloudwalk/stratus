//! In-memory storage implementations.

use std::collections::HashMap;

use parking_lot::RwLock;
use parking_lot::RwLockUpgradableReadGuard;
use parking_lot::RwLockWriteGuard;

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
use crate::eth::storage::AccountWithSlots;
use crate::eth::storage::TemporaryStorage;

#[derive(Debug)]
pub struct InMemoryTemporaryStorage {
    pub pending_block: RwLock<InMemoryTemporaryStorageState>,
    pub latest_block: RwLock<Option<InMemoryTemporaryStorageState>>,
}

impl InMemoryTemporaryStorage {
    pub fn new(block_number: BlockNumber) -> Self {
        Self {
            pending_block: RwLock::new(InMemoryTemporaryStorageState {
                block: PendingBlock::new_at_now(block_number),
                accounts: HashMap::default(),
            }),
            latest_block: RwLock::new(None),
        }
    }

    fn check_conflicts(&self, execution: &EvmExecution) -> anyhow::Result<Option<ExecutionConflicts>> {
        let mut conflicts = ExecutionConflictsBuilder::default();

        for (&address, change) in &execution.changes {
            // check account info conflicts
            if let Some(account) = self.read_account(address)? {
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
                    let Some(original) = self.read_slot(address, slot_index)? else {
                        continue;
                    };
                    if expected.value != original.value {
                        conflicts.add_slot(address, slot_index, original.value, expected.value);
                    }
                }
            }
        }
        Ok(conflicts.build())
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
    pub accounts: HashMap<Address, AccountWithSlots, hash_hasher::HashBuildHasher>,
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

impl TemporaryStorage for InMemoryTemporaryStorage {
    // -------------------------------------------------------------------------
    // Block number
    // -------------------------------------------------------------------------

    // Uneeded clone here, return Cow
    fn read_pending_block_header(&self) -> PendingBlockHeader {
        self.pending_block.read().block.header.clone()
    }

    // -------------------------------------------------------------------------
    // Block and executions
    // -------------------------------------------------------------------------

    fn save_pending_execution(&self, tx: TransactionExecution, check_conflicts: bool) -> Result<(), StratusError> {
        // check conflicts
        let pending_block = self.pending_block.upgradable_read();
        if let TransactionExecution::Local(tx) = &tx {
            if tx.evm_input != (&tx.input, &pending_block.block.header) {
                let expected_input = EvmInput::from_eth_transaction(&tx.input, &pending_block.block.header);
                return Err(StratusError::TransactionEvmInputMismatch {
                    expected: Box::new(expected_input),
                    actual: Box::new(tx.evm_input.clone()),
                });
            }
        }

        let mut pending_block = RwLockUpgradableReadGuard::<InMemoryTemporaryStorageState>::upgrade(pending_block);

        if check_conflicts {
            if let Some(conflicts) = self.check_conflicts(tx.execution())? {
                return Err(StratusError::TransactionConflict(conflicts.into()));
            }
        }

        // save account changes
        let changes = tx.execution().changes.values();
        for change in changes {
            let account = pending_block
                .accounts
                .entry(change.address)
                .or_insert_with(|| AccountWithSlots::new(change.address));

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
        pending_block.block.push_transaction(tx);

        Ok(())
    }

    fn read_pending_executions(&self) -> Vec<TransactionExecution> {
        self.pending_block.read().block.transactions.iter().map(|(_, tx)| tx.clone()).collect()
    }

    fn finish_pending_block(&self) -> anyhow::Result<PendingBlock> {
        let pending_block = self.pending_block.upgradable_read();

        let next_state = InMemoryTemporaryStorageState::new(pending_block.block.header.number.next_block_number());

        let mut pending_block = RwLockUpgradableReadGuard::<InMemoryTemporaryStorageState>::upgrade(pending_block);
        let mut latest = self.latest_block.write();

        *latest = Some(std::mem::replace(&mut *pending_block, next_state));

        drop(pending_block);
        let latest = RwLockWriteGuard::<Option<InMemoryTemporaryStorageState>>::downgrade(latest);

        #[cfg(feature = "dev")]
        let finished_block = {
            let mut finished_block = pending_block
                .as_ref()
                .expect("latest should be Some after finishing the pending block")
                .block
                .clone();
            // Update block timestamp only if evm_setNextBlockTimestamp was called,
            // otherwise keep the original timestamp from pending block creation
            if UnixTime::evm_set_next_block_timestamp_was_called() {
                finished_block.header.timestamp = UnixTimeNow::default();
            }
            finished_block
        };

        #[cfg(not(feature = "dev"))]
        let finished_block = latest.as_ref().expect("latest should be Some after finishing the pending block").block.clone();

        Ok(finished_block)
    }

    fn read_pending_execution(&self, hash: Hash) -> anyhow::Result<Option<TransactionExecution>> {
        let pending_block = self.pending_block.read();
        match pending_block.block.transactions.get(&hash) {
            Some(tx) => Ok(Some(tx.clone())),
            None => Ok(None),
        }
    }

    // -------------------------------------------------------------------------
    // Accounts and Slots
    // -------------------------------------------------------------------------

    fn read_account(&self, address: Address) -> anyhow::Result<Option<Account>> {
        Ok(match self.pending_block.read().accounts.get(&address) {
            Some(pending_account) => Some(pending_account.info.clone()),
            None => self
                .latest_block
                .read()
                .as_ref()
                .and_then(|latest| latest.accounts.get(&address))
                .map(|account| account.info.clone()),
        })
    }

    fn read_slot(&self, address: Address, index: SlotIndex) -> anyhow::Result<Option<Slot>> {
        Ok(
            match self.pending_block.read().accounts.get(&address).and_then(|account| account.slots.get(&index)) {
                Some(pending_slot) => Some(*pending_slot),
                None => self
                    .latest_block
                    .read()
                    .as_ref()
                    .and_then(|latest| latest.accounts.get(&address).and_then(|account| account.slots.get(&index)))
                    .copied(),
            },
        )
    }

    // -------------------------------------------------------------------------
    // Global state
    // -------------------------------------------------------------------------
    fn reset(&self) -> anyhow::Result<()> {
        self.pending_block.write().reset();
        *self.latest_block.write() = None;
        Ok(())
    }
}
