//! In-memory storage implementations.

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
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::ExternalBlock;
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
                block_changes: ExecutionChanges::default(),
            }),
            latest_block: RwLock::new(None),
        }
    }

    pub fn set_pending_from_external(&self, block: &ExternalBlock) {
        let mut pending_block = self.pending_block.write();
        pending_block.block.header.number = block.number();
        pending_block.block.header.timestamp = block.timestamp().into();
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
        if is_local && tx.evm_input != (&tx.input.execution_info, &pending_block.block.header) {
            let expected_input = EvmInput::from_eth_transaction(&tx.input.execution_info, &pending_block.block.header);
            return Err(StorageError::EvmInputMismatch {
                expected: Box::new(expected_input),
                actual: Box::new(tx.evm_input.clone()),
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

    pub fn clone_pending_state(&self) -> InMemoryTemporaryStorageState {
        let pending_block = self.pending_block.read();
        (*pending_block).clone()
    }

    pub fn finish_pending_block(&self) -> anyhow::Result<(PendingBlock, ExecutionChanges), StorageError> {
        let pending_block = self.pending_block.upgradable_read();
        let changes = pending_block.block_changes.clone();

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

        Ok((finished_block, changes))
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
                .latest_block
                .read()
                .as_ref()
                .and_then(|latest| latest.block_changes.accounts.get(&address))
                .map(|account| account.clone().to_account(address)),
        })
    }

    pub fn read_slot(&self, address: Address, index: SlotIndex) -> anyhow::Result<Option<Slot>, StorageError> {
        Ok(match self.pending_block.read().block_changes.slots.get(&(address, index)) {
            Some(pending_value) => Some(Slot::new(index, *pending_value)),
            None => self
                .latest_block
                .read()
                .as_ref()
                .and_then(|latest| latest.block_changes.slots.get(&(address, index)).map(|value| Slot::new(index, *value))),
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
        self.pending_block.write().reset();
        *self.latest_block.write() = None;
        Ok(())
    }
}
