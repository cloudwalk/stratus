//! In-memory storage implementations.

use std::collections::HashMap;

use parking_lot::RwLock;
use parking_lot::RwLockUpgradableReadGuard;
#[cfg(not(feature = "dev"))]
use parking_lot::RwLockWriteGuard;

use crate::eth::executor::EvmInput;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
#[cfg(feature = "dev")]
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Hash;
#[cfg(feature = "dev")]
use crate::eth::primitives::Nonce;
use crate::eth::primitives::PendingBlock;
use crate::eth::primitives::PendingBlockHeader;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StorageError;
use crate::eth::primitives::TransactionExecution;
#[cfg(feature = "dev")]
use crate::eth::primitives::UnixTime;
#[cfg(feature = "dev")]
use crate::eth::primitives::UnixTimeNow;
#[cfg(feature = "dev")]
use crate::eth::primitives::Wei;
use crate::eth::storage::AccountWithSlots;
use crate::eth::storage::TxCount;
use crate::eth::storage::temporary::inmemory::InMemoryTemporaryStorageState;

#[derive(Debug)]
pub struct InmemoryTransactionTemporaryStorage {
    pub pending_block: RwLock<InMemoryTemporaryStorageState>,
    pub latest_block: RwLock<Option<InMemoryTemporaryStorageState>>,
}

impl InmemoryTransactionTemporaryStorage {
    pub fn new(block_number: BlockNumber) -> Self {
        Self {
            pending_block: RwLock::new(InMemoryTemporaryStorageState {
                block: PendingBlock::new_at_now(block_number),
                accounts: HashMap::default(),
            }),
            latest_block: RwLock::new(None),
        }
    }

    pub fn reinit(&self, block_number: BlockNumber) {
        let mut pending = self.pending_block.write();
        let mut latest = self.latest_block.write();
        *pending = InMemoryTemporaryStorageState {
            block: PendingBlock::new_at_now(block_number),
            accounts: HashMap::default(),
        };
        *latest = None;
    }

    // -------------------------------------------------------------------------
    // Block number
    // -------------------------------------------------------------------------

    // Uneeded clone here, return Cow
    pub fn read_pending_block_header(&self) -> (PendingBlockHeader, TxCount) {
        let pending_block = self.pending_block.read();
        (pending_block.block.header.clone(), (pending_block.block.transactions.len() as u64).into())
    }

    #[cfg(feature = "dev")]
    pub fn set_pending_block_header(&self, block_number: BlockNumber) -> anyhow::Result<(), StorageError> {
        self.pending_block.write().block.header.number = block_number;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Block and executions
    // -------------------------------------------------------------------------

    pub fn save_pending_execution(&self, tx: TransactionExecution, is_local: bool) -> Result<(), StorageError> {
        // check conflicts
        let pending_block = self.pending_block.upgradable_read();
        if is_local && tx.evm_input != (&tx.input, &pending_block.block.header) {
            let expected_input = EvmInput::from_eth_transaction(&tx.input, &pending_block.block.header);
            return Err(StorageError::EvmInputMismatch {
                expected: Box::new(expected_input),
                actual: Box::new(tx.evm_input.clone()),
            });
        }

        let mut pending_block = RwLockUpgradableReadGuard::<InMemoryTemporaryStorageState>::upgrade(pending_block);

        // save account changes
        let changes = tx.result.execution.changes.values();
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

    pub fn read_pending_executions(&self) -> Vec<TransactionExecution> {
        self.pending_block.read().block.transactions.iter().map(|(_, tx)| tx.clone()).collect()
    }

    pub fn clone_pending_state(&self) -> InMemoryTemporaryStorageState {
        let pending_block = self.pending_block.read();
        (*pending_block).clone()
    }

    pub fn finish_pending_block(&self) -> anyhow::Result<PendingBlock, StorageError> {
        let pending_block = self.pending_block.upgradable_read();

        // This has to happen BEFORE creating the new state, because UnixTimeNow::default() may change the offset.
        #[cfg(feature = "dev")]
        let finished_block = {
            let mut finished_block = pending_block.block.clone();
            // Update block timestamp only if evm_setNextBlockTimestamp was called,
            // otherwise keep the original timestamp from pending block creation
            if UnixTime::evm_set_next_block_timestamp_was_called() {
                finished_block.header.timestamp = UnixTimeNow::default();
            }
            finished_block
        };

        let next_state = InMemoryTemporaryStorageState::new(pending_block.block.header.number.next_block_number());

        let mut pending_block = RwLockUpgradableReadGuard::<InMemoryTemporaryStorageState>::upgrade(pending_block);
        let mut latest = self.latest_block.write();

        *latest = Some(std::mem::replace(&mut *pending_block, next_state));

        drop(pending_block);

        #[cfg(not(feature = "dev"))]
        let finished_block = {
            let latest = RwLockWriteGuard::<Option<InMemoryTemporaryStorageState>>::downgrade(latest);
            #[allow(clippy::expect_used)]
            latest.as_ref().expect("latest should be Some after finishing the pending block").block.clone()
        };

        Ok(finished_block)
    }

    pub fn read_pending_execution(&self, hash: Hash) -> anyhow::Result<Option<TransactionExecution>, StorageError> {
        let pending_block = self.pending_block.read();
        match pending_block.block.transactions.get(&hash) {
            Some(tx) => Ok(Some(tx.clone())),
            None => Ok(None),
        }
    }

    // -------------------------------------------------------------------------
    // Accounts and Slots
    // -------------------------------------------------------------------------

    pub fn read_account(&self, address: Address) -> anyhow::Result<Option<Account>, StorageError> {
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

    pub fn read_slot(&self, address: Address, index: SlotIndex) -> anyhow::Result<Option<Slot>, StorageError> {
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
    // Direct state manipulation (for testing)
    // -------------------------------------------------------------------------

    #[cfg(feature = "dev")]
    pub fn save_slot(&self, address: Address, slot: Slot) -> anyhow::Result<(), StorageError> {
        let mut pending_block = self.pending_block.write();

        // Only update if the account exists
        if let Some(account) = pending_block.accounts.get_mut(&address) {
            account.slots.insert(slot.index, slot);
        }

        Ok(())
    }

    #[cfg(feature = "dev")]
    pub fn save_account_nonce(&self, address: Address, nonce: Nonce) -> anyhow::Result<(), StorageError> {
        let mut pending_block = self.pending_block.write();

        // Only update if the account exists
        if let Some(account) = pending_block.accounts.get_mut(&address) {
            account.info.nonce = nonce;
        }

        Ok(())
    }

    #[cfg(feature = "dev")]
    pub fn save_account_balance(&self, address: Address, balance: Wei) -> anyhow::Result<(), StorageError> {
        let mut pending_block = self.pending_block.write();

        // Only update if the account exists
        if let Some(account) = pending_block.accounts.get_mut(&address) {
            account.info.balance = balance;
        }

        Ok(())
    }

    #[cfg(feature = "dev")]
    pub fn save_account_code(&self, address: Address, code: Bytes) -> anyhow::Result<(), StorageError> {
        use crate::alias::RevmBytecode;

        let mut pending_block = self.pending_block.write();

        // Only update if the account exists
        if let Some(account) = pending_block.accounts.get_mut(&address) {
            account.info.bytecode = if code.0.is_empty() {
                None
            } else {
                Some(RevmBytecode::new_raw(code.0.into()))
            };
        }

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Global state
    // -------------------------------------------------------------------------
    pub fn reset(&self) -> anyhow::Result<(), StorageError> {
        self.pending_block.write().reset();
        *self.latest_block.write() = None;
        Ok(())
    }
}
