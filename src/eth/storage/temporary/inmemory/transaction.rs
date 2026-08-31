//! In-memory storage implementations.

use parking_lot::RwLock;
use parking_lot::RwLockUpgradableReadGuard;
#[cfg(not(feature = "dev"))]
use parking_lot::RwLockWriteGuard;

use crate::eth::executor::State;
use crate::eth::executor::TransactionExecution;
use crate::eth::executor::TransactionExecutionInput;
use crate::eth::executor::types::state::Complete;
#[cfg(feature = "dev")]
use crate::eth::executor::types::state::CompleteValue;
use crate::eth::storage::StorageError;
use crate::eth::storage::temporary::inmemory::InMemoryTemporaryStorageState;
use crate::eth::types::Account;
use crate::eth::types::Address;
use crate::eth::types::BlockNumber;
#[cfg(feature = "dev")]
use crate::eth::types::Bytes;
use crate::eth::types::Hash;
#[cfg(feature = "dev")]
use crate::eth::types::Nonce;
use crate::eth::types::PendingBlock;
use crate::eth::types::PendingBlockHeader;
use crate::eth::types::Slot;
use crate::eth::types::SlotIndex;
use crate::eth::types::TransactionInput;
use crate::eth::types::UnixTime;
#[cfg(feature = "dev")]
use crate::eth::types::UnixTimeNow;
#[cfg(feature = "dev")]
use crate::eth::types::Wei;

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
                state: State::default(),
            }),
            latest_block: RwLock::new(None),
        }
    }

    pub fn set_pending_header(&self, number: BlockNumber, timestamp: UnixTime) {
        let mut pending_block = self.pending_block.write();
        pending_block.block.header.number = number;
        pending_block.block.header.timestamp = timestamp.into();
    }

    // -------------------------------------------------------------------------
    // Block number
    // -------------------------------------------------------------------------

    pub fn read_pending_block_header(&self) -> PendingBlockHeader {
        let pending_block = self.pending_block.read();
        pending_block.block.header
    }

    #[cfg(feature = "dev")]
    pub fn set_pending_block_header(&self, block_number: BlockNumber) -> anyhow::Result<(), StorageError> {
        self.pending_block.write().block.header.number = block_number;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Block and executions
    // -------------------------------------------------------------------------

    pub fn save_pending_execution(&self, tx: TransactionExecution, state: State<Complete>) -> Result<(), StorageError> {
        // check conflicts
        let pending_block = self.pending_block.upgradable_read();
        if tx.input != &pending_block.block.header {
            let actual_input = tx.input.clone();
            let tx_input: TransactionInput = tx.into();
            let expected_input =
                TransactionExecutionInput::from_eth_transaction(&tx_input, pending_block.block.header.number, *pending_block.block.header.timestamp);
            return Err(StorageError::EvmInputMismatch {
                expected: Box::new(expected_input),
                actual: Box::new(actual_input),
            });
        }

        let mut pending_block = RwLockUpgradableReadGuard::<InMemoryTemporaryStorageState>::upgrade(pending_block);

        pending_block.state.merge(state);

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

    pub fn finish_pending_block(&self) -> (PendingBlock, State<Complete>) {
        let pending_block = self.pending_block.upgradable_read();

        // This has to happen BEFORE creating the new state, because UnixTimeNow::default() may change the offset.
        #[cfg(feature = "dev")]
        let (finished_block, state) = {
            let mut finished_block = pending_block.block.clone();
            // Update block timestamp only if evm_setNextBlockTimestamp was called,
            // otherwise keep the original timestamp from pending block creation
            if UnixTime::evm_set_next_block_timestamp_was_called() {
                finished_block.header.timestamp = UnixTimeNow::default();
            }
            (finished_block, pending_block.state.clone())
        };

        let next_state = InMemoryTemporaryStorageState::new(pending_block.block.header.number.next_block_number());

        let mut pending_block = RwLockUpgradableReadGuard::<InMemoryTemporaryStorageState>::upgrade(pending_block);
        let mut latest = self.latest_block.write();

        *latest = Some(std::mem::replace(&mut *pending_block, next_state));

        drop(pending_block);

        #[cfg(not(feature = "dev"))]
        let (finished_block, state) = {
            let latest = RwLockWriteGuard::<Option<InMemoryTemporaryStorageState>>::downgrade(latest);

            #[allow(clippy::expect_used)]
            let latest_state = latest.as_ref().expect("latest should be Some after finishing the pending block");
            (latest_state.block.clone(), latest_state.state.clone())
        };

        (finished_block, state)
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

    pub fn read_account(&self, address: Address) -> Option<Account> {
        match self.pending_block.read().state.accounts.get(&address) {
            Some(pending_account) => Some(pending_account.clone().to_account(address)),
            None => self
                .latest_block
                .read()
                .as_ref()
                .and_then(|latest| latest.state.accounts.get(&address))
                .map(|account| account.clone().to_account(address)),
        }
    }

    pub fn read_slot(&self, address: Address, index: SlotIndex) -> Option<Slot> {
        match self.pending_block.read().state.slots.get(&(address, index)) {
            Some(pending_value) => Some(Slot::new(index, *pending_value.value())),
            None => self
                .latest_block
                .read()
                .as_ref()
                .and_then(|latest| latest.state.slots.get(&(address, index)).map(|value| Slot::new(index, *value.value()))),
        }
    }

    // -------------------------------------------------------------------------
    // Direct state manipulation (for testing)
    // -------------------------------------------------------------------------

    #[cfg(feature = "dev")]
    pub fn save_slot(&self, address: Address, slot: Slot) -> anyhow::Result<(), StorageError> {
        let mut pending_block = self.pending_block.write();
        pending_block.state.slots.insert((address, slot.index), CompleteValue::Changed(slot.value));
        Ok(())
    }

    #[cfg(feature = "dev")]
    pub fn save_account_nonce(&self, address: Address, nonce: Nonce) -> anyhow::Result<(), StorageError> {
        let mut pending_block = self.pending_block.write();

        // Only update if the account exists
        if let Some(account) = pending_block.state.accounts.get_mut(&address) {
            account.nonce.apply(nonce);
        }

        Ok(())
    }

    #[cfg(feature = "dev")]
    pub fn save_account_balance(&self, address: Address, balance: Wei) -> anyhow::Result<(), StorageError> {
        let mut pending_block = self.pending_block.write();

        // Only update if the account exists
        if let Some(account) = pending_block.state.accounts.get_mut(&address) {
            account.balance.apply(balance);
        }

        Ok(())
    }

    #[cfg(feature = "dev")]
    pub fn save_account_code(&self, address: Address, code: Bytes) -> anyhow::Result<(), StorageError> {
        use crate::alias::RevmBytecode;

        let mut pending_block = self.pending_block.write();

        // Only update if the account exists
        if let Some(account) = pending_block.state.accounts.get_mut(&address) {
            account.bytecode.apply(if code.0.is_empty() {
                None
            } else {
                Some(RevmBytecode::new_raw(code.0.into()))
            });
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
