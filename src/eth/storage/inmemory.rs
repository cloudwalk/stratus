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
    pub accounts: RwLock<HashMap<Address, Account>>,
    pub account_slots: RwLock<HashMap<Address, HashMap<SlotIndex, Slot>>>,
    pub transactions: RwLock<HashMap<Hash, TransactionMined>>,
    pub blocks_by_number: RwLock<IndexMap<BlockNumber, Block>>,
    pub blocks_by_hash: RwLock<IndexMap<Hash, Block>>,
    pub block_number: AtomicUsize,
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        let genesis = BlockMiner::genesis();
        let storage = Self {
            accounts: Default::default(),
            account_slots: Default::default(),
            transactions: Default::default(),
            blocks_by_number: Default::default(),
            blocks_by_hash: Default::default(),
            block_number: Default::default(),
        };
        storage.blocks_by_hash.write().unwrap().insert(genesis.header.hash.clone(), genesis.clone());
        storage.blocks_by_number.write().unwrap().insert(genesis.header.number.clone(), genesis);
        storage
    }
}

impl EthStorage for InMemoryStorage {
    fn read_account(&self, address: &Address) -> Result<Account, EthError> {
        tracing::debug!(%address, "reading account");

        let accounts_lock = self.accounts.read().unwrap();
        match accounts_lock.get(address) {
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

        let account_slots_lock = self.account_slots.read().unwrap();
        let Some(slots) = account_slots_lock.get(address) else {
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

        let block = match selection {
            BlockSelection::Latest => {
                let blocks_lock = self.blocks_by_number.read().unwrap();
                blocks_lock.values().last().cloned()
            }
            BlockSelection::Number(number) => {
                let blocks_lock = self.blocks_by_number.read().unwrap();
                blocks_lock.get(number).cloned()
            }
            BlockSelection::Hash(hash) => {
                let blocks_lock = self.blocks_by_hash.read().unwrap();
                blocks_lock.get(hash).cloned()
            }
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
        let transactions_lock = self.transactions.read().unwrap();
        match transactions_lock.get(hash) {
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
        let mut blocks_by_number_lock = self.blocks_by_number.write().unwrap();
        let mut blocks_by_hash_lock = self.blocks_by_hash.write().unwrap();
        let mut transactions_lock = self.transactions.write().unwrap();
        let mut account_lock = self.accounts.write().unwrap();
        let mut account_slots_lock = self.account_slots.write().unwrap();

        // save block
        tracing::debug!(number = %block.header.number, "saving block");
        blocks_by_number_lock.insert(block.header.number.clone(), block.clone());
        blocks_by_hash_lock.insert(block.header.hash.clone(), block.clone());

        // save transactions
        for transaction in block.transactions {
            tracing::debug!(hash = %transaction.input.hash, "saving transaction");
            transactions_lock.insert(transaction.input.hash.clone(), transaction.clone());

            // save execution changes
            let is_success = transaction.is_success();
            for mut changes in transaction.execution.changes {
                let account = account_lock.entry(changes.address.clone()).or_default();
                let account_slots = account_slots_lock.entry(changes.address).or_default();

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
