//! In-memory storage implementations.

use parking_lot::Mutex;
use parking_lot::MutexGuard;
use parking_lot::RwLock;
use parking_lot::RwLockUpgradableReadGuard;

use crate::eth::executor::EvmInput;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
#[cfg(feature = "dev")]
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::Hash;
#[cfg(feature = "dev")]
use crate::eth::primitives::Nonce;
use crate::eth::primitives::PendingBlock;
use crate::eth::primitives::PendingBlockHeader;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StorageError;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::UnixTime;
use crate::eth::primitives::UnixTimeNow;
#[cfg(feature = "dev")]
use crate::eth::primitives::Wei;
use crate::eth::storage::BlockReference;
use crate::eth::storage::TxCount;
use crate::eth::storage::temporary::inmemory::InMemoryTemporaryStorageState;

#[derive(Debug, Clone)]
pub struct InMemorySealedBlock {
    pub state: InMemoryTemporaryStorageState,
    pub hash: Hash,
}

#[derive(Debug)]
pub struct InMemoryChainState {
    pub pending_block: InMemoryTemporaryStorageState,
    pub latest_sealed: InMemorySealedBlock,
}

pub struct PendingBlockGuard<'a> {
    _guard: MutexGuard<'a, ()>,
}

#[derive(Debug)]
pub struct InmemoryTransactionTemporaryStorage {
    pending_session: Mutex<()>,
    pub state: RwLock<InMemoryChainState>,
}

impl InmemoryTransactionTemporaryStorage {
    pub fn new(latest_sealed: BlockReference) -> Self {
        Self {
            pending_session: Mutex::new(()),
            state: RwLock::new(InMemoryChainState {
                pending_block: InMemoryTemporaryStorageState {
                    block: PendingBlock::new_at_now(latest_sealed.number.next_block_number()),
                    block_changes: ExecutionChanges::default(),
                },
                latest_sealed: InMemorySealedBlock {
                    state: InMemoryTemporaryStorageState::new_sealed(latest_sealed.number),
                    hash: latest_sealed.hash,
                },
            }),
        }
    }

    pub(super) fn pending_block_guard(&self) -> PendingBlockGuard<'_> {
        PendingBlockGuard {
            _guard: self.pending_session.lock(),
        }
    }

    pub(super) fn read_latest_sealed(&self) -> BlockReference {
        let state = self.state.read();
        BlockReference {
            number: state.latest_sealed.state.block.header.number,
            hash: state.latest_sealed.hash,
        }
    }

    pub fn set_pending_header(&self, number: BlockNumber, timestamp: UnixTime) {
        let mut state = self.state.write();
        state.pending_block.block.header.number = number;
        state.pending_block.block.header.timestamp = timestamp.into();
    }

    // -------------------------------------------------------------------------
    // Block number
    // -------------------------------------------------------------------------

    // Uneeded clone here, return Cow
    pub fn read_pending_block_header(&self) -> (PendingBlockHeader, TxCount) {
        let state = self.state.read();
        (
            state.pending_block.block.header.clone(),
            (state.pending_block.block.transactions.len() as u64).into(),
        )
    }

    #[cfg(feature = "dev")]
    pub fn set_pending_block_header(&self, block_number: BlockNumber) -> anyhow::Result<(), StorageError> {
        self.state.write().pending_block.block.header.number = block_number;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Block and executions
    // -------------------------------------------------------------------------

    pub fn save_pending_execution(&self, tx: TransactionExecution) -> Result<(), StorageError> {
        // check conflicts
        let state = self.state.upgradable_read();
        if tx.evm_input != &state.pending_block.block.header {
            let actual_input = tx.evm_input.clone();
            let tx_input: TransactionInput = tx.into();
            let expected_input =
                EvmInput::from_eth_transaction(&tx_input, state.pending_block.block.header.number, *state.pending_block.block.header.timestamp);
            return Err(StorageError::EvmInputMismatch {
                expected: Box::new(expected_input),
                actual: Box::new(actual_input),
            });
        }

        let mut state = RwLockUpgradableReadGuard::<InMemoryChainState>::upgrade(state);

        state.pending_block.block_changes.merge(tx.result.execution.changes.clone()); // TODO: This clone can be removed by reworking the primitives

        // save execution
        state.pending_block.block.push_transaction(tx);

        Ok(())
    }

    pub fn read_pending_executions(&self) -> Vec<TransactionExecution> {
        self.state.read().pending_block.block.transactions.iter().map(|(_, tx)| tx.clone()).collect()
    }

    pub fn pending_block_to_seal(&self) -> (PendingBlock, ExecutionChanges) {
        let state = self.state.read();
        let block = state.pending_block.block.clone();

        // This has to happen before creating the next state because UnixTimeNow::default() may change the offset.
        #[cfg(feature = "dev")]
        let block = {
            let mut block = block;
            // Update the timestamp only if evm_setNextBlockTimestamp was called.
            if UnixTime::evm_set_next_block_timestamp_was_called() {
                block.header.timestamp = UnixTimeNow::default();
            }
            block
        };

        (block, state.pending_block.block_changes.clone())
    }

    pub(super) fn finish_pending_block(&self, block: BlockReference, timestamp: UnixTimeNow) {
        let next_state = InMemoryTemporaryStorageState::new(block.number.next_block_number());
        let mut state = self.state.write();

        debug_assert_eq!(state.pending_block.block.header.number, block.number);
        let mut finished_state = std::mem::replace(&mut state.pending_block, next_state);
        finished_state.block.header.timestamp = timestamp;
        state.latest_sealed = InMemorySealedBlock {
            state: finished_state,
            hash: block.hash,
        };
    }

    pub fn read_pending_execution(&self, hash: Hash) -> anyhow::Result<Option<TransactionExecution>, StorageError> {
        let state = self.state.read();
        match state.pending_block.block.transactions.get(&hash) {
            Some(tx) => Ok(Some(tx.clone())),
            None => Ok(None),
        }
    }

    // -------------------------------------------------------------------------
    // Accounts and Slots
    // -------------------------------------------------------------------------

    pub fn read_account(&self, address: Address) -> anyhow::Result<Option<Account>, StorageError> {
        let state = self.state.read();
        Ok(match state.pending_block.block_changes.accounts.get(&address) {
            Some(pending_account) => Some(pending_account.clone().to_account(address)),
            None => state
                .latest_sealed
                .state
                .block_changes
                .accounts
                .get(&address)
                .map(|account| account.clone().to_account(address)),
        })
    }

    pub fn read_slot(&self, address: Address, index: SlotIndex) -> anyhow::Result<Option<Slot>, StorageError> {
        let state = self.state.read();
        Ok(match state.pending_block.block_changes.slots.get(&(address, index)) {
            Some(pending_value) => Some(Slot::new(index, *pending_value)),
            None => state
                .latest_sealed
                .state
                .block_changes
                .slots
                .get(&(address, index))
                .map(|value| Slot::new(index, *value)),
        })
    }

    // -------------------------------------------------------------------------
    // Direct state manipulation (for testing)
    // -------------------------------------------------------------------------

    #[cfg(feature = "dev")]
    pub fn save_slot(&self, address: Address, slot: Slot) -> anyhow::Result<(), StorageError> {
        let mut state = self.state.write();
        state.pending_block.block_changes.slots.insert((address, slot.index), slot.value);
        Ok(())
    }

    #[cfg(feature = "dev")]
    pub fn save_account_nonce(&self, address: Address, nonce: Nonce) -> anyhow::Result<(), StorageError> {
        let mut state = self.state.write();

        // Only update if the account exists
        if let Some(account) = state.pending_block.block_changes.accounts.get_mut(&address) {
            account.nonce.apply(nonce);
        }

        Ok(())
    }

    #[cfg(feature = "dev")]
    pub fn save_account_balance(&self, address: Address, balance: Wei) -> anyhow::Result<(), StorageError> {
        let mut state = self.state.write();

        // Only update if the account exists
        if let Some(account) = state.pending_block.block_changes.accounts.get_mut(&address) {
            account.balance.apply(balance);
        }

        Ok(())
    }

    #[cfg(feature = "dev")]
    pub fn save_account_code(&self, address: Address, code: Bytes) -> anyhow::Result<(), StorageError> {
        use crate::alias::RevmBytecode;

        let mut state = self.state.write();

        // Only update if the account exists
        if let Some(account) = state.pending_block.block_changes.accounts.get_mut(&address) {
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
        let genesis = BlockReference::genesis();
        let mut state = self.state.write();
        state.pending_block.reset();
        state.latest_sealed = InMemorySealedBlock {
            state: InMemoryTemporaryStorageState::new_sealed(genesis.number),
            hash: genesis.hash,
        };
        Ok(())
    }
}
