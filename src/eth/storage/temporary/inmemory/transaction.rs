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
use crate::eth::storage::temporary::journal::TransactionJournal;
use crate::eth::types::Account;
use crate::eth::types::Address;
use crate::eth::types::BlockInfo;
use crate::eth::types::BlockNumber;
#[cfg(feature = "dev")]
use crate::eth::types::Bytes;
use crate::eth::types::Hash;
#[cfg(feature = "dev")]
use crate::eth::types::Nonce;
use crate::eth::types::PendingBlock;
use crate::eth::types::Slot;
use crate::eth::types::SlotIndex;
use crate::eth::types::TransactionInput;
use crate::eth::types::TransactionMined;
use crate::eth::types::TransactionStage;
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
            pending_block: RwLock::new(InMemoryTemporaryStorageState::new(block_number)),
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

    pub fn read_pending_block_header(&self) -> BlockInfo {
        let pending_block = self.pending_block.read();
        pending_block.block.header
    }

    // -------------------------------------------------------------------------
    // Block and executions
    // -------------------------------------------------------------------------

    pub fn save_pending_execution(&self, tx: TransactionExecution, state: State<Complete>) -> Result<(), StorageError> {
        // Serialize persistence with block rotation while allowing pending-state reads during disk synchronization.
        let pending_block = self.pending_block.upgradable_read();
        if tx.input != pending_block.block.header {
            let actual_input = tx.input.clone();
            let tx_input: TransactionInput = tx.into();
            let expected_input = TransactionExecutionInput::create(&tx_input, pending_block.block.header);
            return Err(StorageError::EvmInputMismatch {
                expected: Box::new(expected_input),
                actual: Box::new(actual_input),
            });
        }

        let mut pending_block = RwLockUpgradableReadGuard::<InMemoryTemporaryStorageState>::upgrade(pending_block);

        if let Some(journal) = pending_block.journal.as_mut() {
            let _ = journal.append(&tx).inspect_err(|err| tracing::error!(?err, "append to journal failed"));
        }
        pending_block.state.merge(state);
        pending_block.block.push_transaction(tx);
        drop(pending_block);

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
        let dropped_block_number = latest.as_ref().map(|state| state.block.header.number);

        *latest = Some(std::mem::replace(&mut *pending_block, next_state));

        drop(pending_block);

        #[cfg(not(feature = "dev"))]
        let (finished_block, state) = {
            let latest = RwLockWriteGuard::<Option<InMemoryTemporaryStorageState>>::downgrade(latest);

            #[allow(clippy::expect_used)]
            let latest_state = latest.as_ref().expect("latest should be Some after finishing the pending block");
            (latest_state.block.clone(), latest_state.state.clone())
        };

        if let Some(number) = dropped_block_number {
            let _ = TransactionJournal::retire(number).inspect_err(|err| tracing::error!(?err, "failed to retire journal"));
        }

        (finished_block, state)
    }

    // i think this is used only for eth_transactionByHash and getTransactionReceipt so we dont really need to clone the entire execution
    pub fn read_transaction(&self, hash: Hash) -> Option<TransactionStage> {
        let pending = self.pending_block.read();
        if let Some(execution) = pending.block.transactions.get(&hash) {
            return Some(TransactionStage::Pending(execution.clone()));
        }

        let latest = self.latest_block.read();
        let block = &latest.as_ref()?.block;
        let (index, _, execution) = block.transactions.get_full(&hash)?;
        let first_log_index: u64 = block.transactions.values().take(index).map(|tx| tx.logs().len() as u64).sum();
        Some(TransactionStage::Mined(TransactionMined::from_execution(
            execution.clone(),
            block.header.number.hash(),
            (index as u64).into(),
            first_log_index.into(),
        )))
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

    /// Retains only the addresses that are missing from both the pending and latest temporary states.
    ///
    /// Batched: each lock is acquired once for the whole key set, instead of once per key,
    /// to reduce contention with the executor.
    pub fn retain_missing_accounts(&self, addresses: &mut Vec<Address>) {
        {
            let pending_block = self.pending_block.read();
            addresses.retain(|address| !pending_block.state.accounts.contains_key(address));
        }
        let latest_block = self.latest_block.read();
        if let Some(latest_block) = latest_block.as_ref() {
            addresses.retain(|address| !latest_block.state.accounts.contains_key(address));
        }
    }

    /// Retains only the slot keys that are missing from both the pending and latest temporary states.
    ///
    /// Batched: each lock is acquired once for the whole key set, instead of once per key,
    /// to reduce contention with the executor.
    pub fn retain_missing_slots(&self, slot_keys: &mut Vec<(Address, SlotIndex)>) {
        {
            let pending_block = self.pending_block.read();
            slot_keys.retain(|slot_key| !pending_block.state.slots.contains_key(slot_key));
        }
        let latest_block = self.latest_block.read();
        if let Some(latest_block) = latest_block.as_ref() {
            slot_keys.retain(|slot_key| !latest_block.state.slots.contains_key(slot_key));
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
    #[cfg(feature = "dev")]
    pub fn reset(&self) {
        let mut pending = self.pending_block.write();
        pending.reset();
        let header = pending.block.header;
        *self.latest_block.write() = None;
    }
}
