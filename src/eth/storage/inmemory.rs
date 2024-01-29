//! In-memory storage implementations.

use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use async_trait::async_trait;
use indexmap::IndexMap;
use tokio::sync::RwLock;
use tokio::sync::RwLockReadGuard;
use tokio::sync::RwLockWriteGuard;

use crate::eth::miner::BlockMiner;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Hash;
use crate::eth::primitives::HistoricalValues;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionExecutionConflicts;
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::Wei;
use crate::eth::storage::test_accounts;
use crate::eth::storage::EthStorage;
use crate::eth::storage::EthStorageError;

/// In-memory implementation using maps.
#[derive(Debug)]
pub struct InMemoryStorage {
    state: RwLock<InMemoryStorageState>,
    block_number: AtomicUsize,
}

impl InMemoryStorage {
    /// Locks inner state for reading.
    async fn lock_read(&self) -> RwLockReadGuard<'_, InMemoryStorageState> {
        self.state.read().await
    }

    /// Locks inner state for writing.
    async fn lock_write(&self) -> RwLockWriteGuard<'_, InMemoryStorageState> {
        self.state.write().await
    }
}

#[derive(Debug, Default)]
struct InMemoryStorageState {
    accounts: HashMap<Address, (Account, HistoricalValues<Wei>)>,
    account_slots: HashMap<Address, HashMap<SlotIndex, HistoricalValues<Slot>>>,
    transactions: HashMap<Hash, TransactionMined>,
    blocks_by_number: IndexMap<BlockNumber, Block>,
    blocks_by_hash: IndexMap<Hash, Block>,
    logs: Vec<LogMined>,
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        let mut state = InMemoryStorageState::default();

        // add genesis block to state
        let genesis = BlockMiner::genesis();
        state.blocks_by_hash.insert(genesis.header.hash.clone(), genesis.clone());
        state.blocks_by_number.insert(genesis.header.number, genesis);

        // add test accounts to state
        for account in test_accounts() {
            let balance = account.balance.clone();
            state
                .accounts
                .insert(account.address.clone(), (account.clone(), HistoricalValues::new(BlockNumber::ZERO, balance)));
        }

        Self {
            state: RwLock::new(state),
            block_number: Default::default(),
        }
    }
}

#[async_trait]
impl EthStorage for InMemoryStorage {
    // -------------------------------------------------------------------------
    // Block number operations
    // -------------------------------------------------------------------------

    async fn read_current_block_number(&self) -> anyhow::Result<BlockNumber> {
        Ok(self.block_number.load(Ordering::SeqCst).into())
    }

    async fn increment_block_number(&self) -> anyhow::Result<BlockNumber> {
        let next = self.block_number.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(next.into())
    }

    // -------------------------------------------------------------------------
    // State operations
    // ------------------------------------------------------------------------

    async fn check_conflicts(&self, execution: &TransactionExecution) -> anyhow::Result<TransactionExecutionConflicts> {
        let state_lock = self.state.read().await;
        Ok(check_conflicts(&state_lock, execution))
    }

    async fn read_account(&self, address: &Address, point_in_time: &StoragePointInTime) -> anyhow::Result<Account> {
        tracing::debug!(%address, "reading account");

        let state_lock = self.lock_read().await;

        match state_lock.accounts.get(address) {
            // account found
            Some((account, balance_history)) => {
                let balance = balance_history.get_at_point(point_in_time).unwrap_or_default();
                tracing::trace!(%address, %balance, "account found");
                Ok(Account {
                    address: address.clone(),
                    balance,
                    ..account.clone()
                })
            }

            // account not found
            None => {
                tracing::trace!(%address, "account not found");
                Ok(Account {
                    address: address.clone(),
                    ..Account::default()
                })
            }
        }
    }

    async fn read_slot(&self, address: &Address, slot_index: &SlotIndex, point_in_time: &StoragePointInTime) -> anyhow::Result<Slot> {
        tracing::debug!(%address, %slot_index, ?point_in_time, "reading slot");

        let state_lock = self.lock_read().await;
        let Some(slots) = state_lock.account_slots.get(address) else {
            tracing::trace!(%address, "account slot not found");
            return Ok(Default::default());
        };

        match slots.get(slot_index) {
            // slot exists and block NOT specified
            Some(slot_history) => {
                let slot = slot_history.get_at_point(point_in_time).unwrap_or_default();
                tracing::trace!(%address, %slot_index, %slot, "slot found");
                Ok(slot)
            }

            // slot NOT exists
            None => {
                tracing::trace!(%address, ?point_in_time, %slot_index, "slot not found");
                Ok(Slot::default())
            }
        }
    }

    async fn read_block(&self, selection: &BlockSelection) -> anyhow::Result<Option<Block>> {
        tracing::debug!(?selection, "reading block");

        let state_lock = self.lock_read().await;
        let block = match selection {
            BlockSelection::Latest => state_lock.blocks_by_number.values().last().cloned(),
            BlockSelection::Earliest => state_lock.blocks_by_number.values().next().cloned(),
            BlockSelection::Number(number) => state_lock.blocks_by_number.get(number).cloned(),
            BlockSelection::Hash(hash) => state_lock.blocks_by_hash.get(hash).cloned(),
        };
        match block {
            Some(block) => {
                tracing::trace!(?selection, ?block, "block found");
                Ok(Some(block.clone()))
            }
            None => {
                tracing::trace!(?selection, "block not found");
                Ok(None)
            }
        }
    }

    async fn read_mined_transaction(&self, hash: &Hash) -> anyhow::Result<Option<TransactionMined>> {
        tracing::debug!(%hash, "reading transaction");
        let state_lock = self.lock_read().await;

        match state_lock.transactions.get(hash) {
            Some(transaction) => {
                tracing::trace!(%hash, "transaction found");
                Ok(Some(transaction.clone()))
            }
            None => {
                tracing::trace!(%hash, "transaction not found");
                Ok(None)
            }
        }
    }

    async fn read_logs(&self, filter: &LogFilter) -> anyhow::Result<Vec<LogMined>> {
        tracing::debug!(?filter, "reading logs");
        let state_lock = self.lock_read().await;

        let logs = state_lock
            .logs
            .iter()
            .skip_while(|log| log.block_number < filter.from_block)
            .take_while(|log| match filter.to_block {
                Some(to_block) => log.block_number <= to_block,
                None => true,
            })
            .filter(|log| filter.matches(log))
            .cloned()
            .collect();
        Ok(logs)
    }

    async fn save_block(&self, block: Block) -> anyhow::Result<(), EthStorageError> {
        let mut state_lock = self.lock_write().await;

        // check conflicts
        for transaction in &block.transactions {
            let conflicts = check_conflicts(&state_lock, &transaction.execution);
            if conflicts.any() {
                return Err(EthStorageError::Conflict(conflicts));
            }
        }

        // save block
        tracing::debug!(number = %block.header.number, "saving block");
        state_lock.blocks_by_number.insert(block.header.number, block.clone());
        state_lock.blocks_by_hash.insert(block.header.hash.clone(), block.clone());

        // save transactions
        for transaction in block.transactions {
            tracing::debug!(hash = %transaction.input.hash, "saving transaction");
            state_lock.transactions.insert(transaction.input.hash.clone(), transaction.clone());
            let is_success = transaction.is_success();

            // save logs
            if is_success {
                for log in transaction.logs {
                    state_lock.logs.push(log);
                }
            }

            // save execution changes
            for changes in transaction.execution.changes {
                let (account, account_balances) = state_lock
                    .accounts
                    .entry(changes.address.clone())
                    .or_insert_with(|| (Account::default(), HistoricalValues::new(BlockNumber::ZERO, Wei::ZERO)));

                // nonce
                if let Some(nonce) = changes.nonce.take_if_modified() {
                    account.nonce = nonce;
                }

                // balance
                if let Some(balance) = changes.balance.take_if_modified() {
                    account.balance = balance.clone();
                    account_balances.push(block.header.number, balance);
                }

                // bytecode
                if is_success {
                    if let Some(Some(bytecode)) = changes.bytecode.take_if_modified() {
                        tracing::trace!(bytecode_len = %bytecode.len(), "saving bytecode");
                        account.bytecode = Some(bytecode);
                    }
                }

                // slots
                if is_success {
                    let account_slots = state_lock.account_slots.entry(changes.address).or_default();
                    for (slot_index, slot) in changes.slots {
                        if let Some(slot) = slot.take_if_modified() {
                            tracing::trace!(%slot, "saving slot");
                            match account_slots.get_mut(&slot_index) {
                                Some(slot_history) => {
                                    slot_history.push(block.header.number, slot);
                                }
                                None => {
                                    account_slots.insert(slot_index, HistoricalValues::new(block.header.number, slot));
                                }
                            };
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

fn check_conflicts(state: &InMemoryStorageState, execution: &TransactionExecution) -> TransactionExecutionConflicts {
    let mut conflicts = TransactionExecutionConflicts::default();

    for change in &execution.changes {
        let address = &change.address;

        // check account info conflicts
        match state.accounts.get(address) {
            Some((account, _)) => {
                if let Some(touched_nonce) = change.nonce.take_original_ref() {
                    if *touched_nonce != account.nonce {
                        conflicts.add_nonce(address.clone(), account.nonce.clone(), touched_nonce.clone());
                    }
                }
                if let Some(touched_balance) = change.balance.take_original_ref() {
                    if *touched_balance != account.balance {
                        conflicts.add_balance(address.clone(), account.balance.clone(), touched_balance.clone());
                    }
                }
            }
            None => {
                // todo: handle account creation conflicts
            }
        }

        // check slots conflicts
        match state.account_slots.get(address) {
            Some(slots) => {
                for (touched_slot_index, touched_slot) in &change.slots {
                    match slots.get(touched_slot_index) {
                        Some(slot) =>
                            if let Some(touched_slot) = touched_slot.take_original_ref() {
                                let slot_value = slot.get_current().value;
                                if touched_slot.value != slot_value {
                                    conflicts.add_slot(address.clone(), touched_slot_index.clone(), slot_value, touched_slot.value.clone());
                                }
                            },
                        None => {
                            // todo: handle slot creation conflict
                        }
                    }
                }
            }
            None => {
                // todo: handle account creation scenarios
            }
        }
    }

    conflicts
}
