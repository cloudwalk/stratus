//! In-memory storage implementations.

use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::RwLock;

use indexmap::IndexMap;

use crate::eth::miner::BlockMiner;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::BlockNumberStorage;
use crate::eth::storage::EthStorage;
use crate::eth::EthError;

/// In-memory implementation using HashMaps.
#[derive(Debug)]
pub struct InMemoryStorage {
    state: RwLock<State>,
    block_number: AtomicUsize,
}

#[derive(Debug, Default)]
struct State {
    accounts: HashMap<Address, Account>,
    account_slots: HashMap<Address, HashMap<SlotIndex, Slot>>,
    transactions: HashMap<Hash, TransactionMined>,
    blocks_by_number: IndexMap<BlockNumber, Block>,
    blocks_by_hash: IndexMap<Hash, Block>,
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        let genesis = BlockMiner::genesis();
        let mut state = State::default();
        state.blocks_by_hash.insert(genesis.header.hash.clone(), genesis.clone());
        state.blocks_by_number.insert(genesis.header.number.clone(), genesis);

        Self {
            state: RwLock::new(state),
            block_number: Default::default(),
        }
    }
}

impl EthStorage for InMemoryStorage {
    fn read_account(&self, address: &Address) -> Result<Account, EthError> {
        tracing::debug!(%address, "reading account");

        let state_lock = self.state.read().unwrap();
        match state_lock.accounts.get(address) {
            Some(account) => {
                let bytecode_len = account.bytecode.as_ref().map(|x| x.len()).unwrap_or_default();
                tracing::trace!(%address, %bytecode_len, "account found");
                Ok(Account {
                    address: address.clone(),
                    ..account.clone()
                })
            }
            None => {
                tracing::trace!(%address, "account not found");
                Ok(Account {
                    address: address.clone(),
                    ..Default::default()
                })
            }
        }
    }

    fn read_slot(&self, address: &Address, slot_index: &SlotIndex) -> Result<Slot, EthError> {
        tracing::debug!(%address, %slot_index, "reading slot");

        let state_lock = self.state.read().unwrap();
        let Some(slots) = state_lock.account_slots.get(address) else {
            tracing::trace!(%address, "account slot not found");
            return Ok(Default::default());
        };
        match slots.get(slot_index) {
            Some(slot) => {
                tracing::trace!(%address, %slot_index, %slot, "slot found");
                Ok(slot.clone())
            }
            None => {
                tracing::trace!(%address, %slot_index, "account found, but slot not found");
                Ok(Default::default())
            }
        }
    }

    fn read_block(&self, selection: &BlockSelection) -> Result<Option<Block>, EthError> {
        tracing::debug!(?selection, "reading block");

        let state_lock = self.state.read().unwrap();
        let block = match selection {
            BlockSelection::Latest => state_lock.blocks_by_number.values().last().cloned(),
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

    fn read_mined_transaction(&self, hash: &Hash) -> Result<Option<TransactionMined>, EthError> {
        tracing::debug!(%hash, "reading transaction");
        let state_lock = self.state.read().unwrap();

        match state_lock.transactions.get(hash) {
            Some(transaction) => {
                tracing::trace!(%hash, ?transaction, "transaction found");
                Ok(Some(transaction.clone()))
            }
            None => {
                tracing::trace!(%hash, "transaction not found");
                Ok(None)
            }
        }
    }

    fn save_block(&self, block: Block) -> Result<(), EthError> {
        let mut state_lock = self.state.write().unwrap();

        // save block
        tracing::debug!(number = %block.header.number, "saving block");
        state_lock.blocks_by_number.insert(block.header.number.clone(), block.clone());
        state_lock.blocks_by_hash.insert(block.header.hash.clone(), block.clone());

        // save transactions
        for transaction in block.transactions {
            tracing::debug!(hash = %transaction.input.hash, "saving transaction");
            state_lock.transactions.insert(transaction.input.hash.clone(), transaction.clone());

            // save execution changes
            let is_success = transaction.is_success();
            for mut changes in transaction.execution.changes {
                let account = state_lock.accounts.entry(changes.address.clone()).or_default();

                // nonce
                if let Some(nonce) = changes.nonce.take_if_modified() {
                    account.nonce = nonce;
                }

                // balance
                if let Some(balance) = changes.balance.take_if_modified() {
                    account.balance = balance;
                }

                // bytecode
                if is_success {
                    if let Some(Some(bytecode)) = changes.bytecode.take_if_modified() {
                        tracing::trace!(bytecode_len = %bytecode.len(), "saving bytecode");
                        account.bytecode = Some(bytecode);
                    }
                }

                // storage
                if is_success {
                    let account_slots = state_lock.account_slots.entry(changes.address).or_default();
                    for (slot_index, mut slot) in changes.slots {
                        if let Some(slot) = slot.take_if_modified() {
                            tracing::trace!(%slot, "saving slot");
                            account_slots.insert(slot_index, slot);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

impl BlockNumberStorage for InMemoryStorage {
    fn current_block_number(&self) -> Result<BlockNumber, EthError> {
        Ok(self.block_number.load(Ordering::SeqCst).into())
    }

    fn increment_block_number(&self) -> Result<BlockNumber, EthError> {
        let next = self.block_number.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(next.into())
    }
}
