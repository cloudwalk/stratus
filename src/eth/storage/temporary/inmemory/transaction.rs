//! In-memory storage implementations.

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
pub struct InmemoryTransactionTemporaryStorage {
    pub pending_block: RwLock<InMemoryTemporaryStorageState>,
    pub latest_sealed: RwLock<InMemorySealedBlock>,
}

impl InmemoryTransactionTemporaryStorage {
    pub fn new(latest_sealed: BlockReference) -> Self {
        Self {
            pending_block: RwLock::new(InMemoryTemporaryStorageState {
                block: PendingBlock::new_at_now(latest_sealed.number.next_block_number()),
                block_changes: ExecutionChanges::default(),
            }),
            latest_sealed: RwLock::new(InMemorySealedBlock {
                state: InMemoryTemporaryStorageState::new_sealed(latest_sealed.number),
                hash: latest_sealed.hash,
            }),
        }
    }

    pub(super) fn read_latest_sealed(&self) -> BlockReference {
        let latest = self.latest_sealed.read();
        BlockReference {
            number: latest.state.block.header.number,
            hash: latest.hash,
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

    pub fn save_pending_execution(&self, tx: TransactionExecution) -> Result<(), StorageError> {
        // check conflicts
        let pending_block = self.pending_block.upgradable_read();
        if tx.evm_input != &pending_block.block.header {
            let actual_input = tx.evm_input.clone();
            let tx_input: TransactionInput = tx.into();
            let expected_input = EvmInput::from_eth_transaction(&tx_input, pending_block.block.header.number, *pending_block.block.header.timestamp);
            return Err(StorageError::EvmInputMismatch {
                expected: Box::new(expected_input),
                actual: Box::new(actual_input),
            });
        }

        let mut pending_block = RwLockUpgradableReadGuard::<InMemoryTemporaryStorageState>::upgrade(pending_block);

        pending_block.block_changes.merge(tx.result.execution.changes.clone()); // TODO: This clone can be removed by reworking the primitives

        // save execution
        pending_block.block.push_transaction(tx);

        Ok(())
    }

    pub fn read_pending_executions(&self) -> Vec<TransactionExecution> {
        self.pending_block.read().block.transactions.iter().map(|(_, tx)| tx.clone()).collect()
    }

    pub fn pending_block_to_seal(&self) -> (PendingBlock, ExecutionChanges) {
        let pending_block = self.pending_block.read();
        let block = pending_block.block.clone();

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

        (block, pending_block.block_changes.clone())
    }

    pub(super) fn finish_pending_block(&self, block: BlockReference, timestamp: UnixTimeNow) {
        let next_state = InMemoryTemporaryStorageState::new(block.number.next_block_number());
        let mut pending_block = self.pending_block.write();
        let mut latest_sealed = self.latest_sealed.write();

        debug_assert_eq!(pending_block.block.header.number, block.number);
        let mut finished_state = std::mem::replace(&mut *pending_block, next_state);
        finished_state.block.header.timestamp = timestamp;
        *latest_sealed = InMemorySealedBlock {
            state: finished_state,
            hash: block.hash,
        };
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
        Ok(match self.pending_block.read().block_changes.accounts.get(&address) {
            Some(pending_account) => Some(pending_account.clone().to_account(address)),
            None => self
                .latest_sealed
                .read()
                .state
                .block_changes
                .accounts
                .get(&address)
                .map(|account| account.clone().to_account(address)),
        })
    }

    pub fn read_slot(&self, address: Address, index: SlotIndex) -> anyhow::Result<Option<Slot>, StorageError> {
        Ok(match self.pending_block.read().block_changes.slots.get(&(address, index)) {
            Some(pending_value) => Some(Slot::new(index, *pending_value)),
            None => self
                .latest_sealed
                .read()
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
        let mut pending_block = self.pending_block.write();
        pending_block.block_changes.slots.insert((address, slot.index), slot.value);
        Ok(())
    }

    #[cfg(feature = "dev")]
    pub fn save_account_nonce(&self, address: Address, nonce: Nonce) -> anyhow::Result<(), StorageError> {
        let mut pending_block = self.pending_block.write();

        // Only update if the account exists
        if let Some(account) = pending_block.block_changes.accounts.get_mut(&address) {
            account.nonce.apply(nonce);
        }

        Ok(())
    }

    #[cfg(feature = "dev")]
    pub fn save_account_balance(&self, address: Address, balance: Wei) -> anyhow::Result<(), StorageError> {
        let mut pending_block = self.pending_block.write();

        // Only update if the account exists
        if let Some(account) = pending_block.block_changes.accounts.get_mut(&address) {
            account.balance.apply(balance);
        }

        Ok(())
    }

    #[cfg(feature = "dev")]
    pub fn save_account_code(&self, address: Address, code: Bytes) -> anyhow::Result<(), StorageError> {
        use crate::alias::RevmBytecode;

        let mut pending_block = self.pending_block.write();

        // Only update if the account exists
        if let Some(account) = pending_block.block_changes.accounts.get_mut(&address) {
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
        self.pending_block.write().reset();
        *self.latest_sealed.write() = InMemorySealedBlock {
            state: InMemoryTemporaryStorageState::new_sealed(genesis.number),
            hash: genesis.hash,
        };
        Ok(())
    }
}
