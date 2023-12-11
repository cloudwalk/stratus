//! In-memory storage implementations.

use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::RwLock;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::Block;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::BlockNumberStorage;
use crate::eth::storage::EthStorage;
use crate::eth::EthError;

/// In-memory implementation using HashMaps.
#[derive(Debug, Default)]
pub struct InMemoryStorage {
    pub accounts: RwLock<HashMap<Address, Account>>,
    pub account_slots: RwLock<HashMap<Address, HashMap<SlotIndex, Slot>>>,
    pub transactions: RwLock<HashMap<Hash, TransactionMined>>,
    pub blocks: RwLock<HashMap<BlockNumber, Block>>,
    pub block_number: AtomicUsize,
}

impl EthStorage for InMemoryStorage {
    fn read_account(&self, address: &Address) -> Result<Account, EthError> {
        tracing::debug!(%address, "reading account");

        let lock = self.accounts.read().unwrap();
        match lock.get(address) {
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

        let lock = self.account_slots.read().unwrap();
        let slots = match lock.get(address) {
            Some(slots) => slots,
            None => {
                tracing::trace!(%address, "account slot not found");
                return Ok(Default::default());
            }
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

    fn read_block(&self, number: &BlockNumber) -> Result<Option<Block>, EthError> {
        tracing::debug!(%number, "reading block");

        // handle genesis block
        if number.is_genesis() {
            // TODO: genesis block
            return Ok(Some(Block::default()));
        }

        // handle other blocks
        let lock = self.blocks.read().unwrap();
        match lock.get(number) {
            Some(block) => {
                tracing::trace!(%number, ?block, "block found");
                Ok(Some(block.clone()))
            }
            None => {
                tracing::trace!(%number, "block not found");
                Ok(None)
            }
        }
    }

    fn read_mined_transaction(&self, hash: &Hash) -> Result<Option<TransactionMined>, EthError> {
        tracing::debug!(%hash, "reading transaction");
        let lock = self.transactions.read().unwrap();
        match lock.get(hash) {
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

    fn save_mined_transaction(&self, mined: TransactionMined) -> Result<(), EthError> {
        let mut account_lock = self.accounts.write().unwrap();
        let mut account_slots_lock = self.account_slots.write().unwrap();
        let mut transactions_lock = self.transactions.write().unwrap();
        let mut blocks_lock = self.blocks.write().unwrap();

        // save transaction
        tracing::debug!(hash = %mined.transaction_input.hash, "saving transaction");
        blocks_lock.insert(mined.block.number.clone(), mined.block.clone());
        transactions_lock.insert(mined.transaction_input.hash.clone(), mined.clone());

        // save execution changes
        let completed = mined.is_commited();
        let execution_changes = mined.execution.changes;
        for mut changes in execution_changes {
            let address = changes.address;
            tracing::debug!(%address, "saving account changes");
            let account = account_lock.entry(address.clone()).or_default();
            let account_slots = account_slots_lock.entry(address).or_default();

            // nonce
            if let Some(nonce) = changes.nonce.take_if_modified() {
                tracing::trace!(%nonce, "saving nonce");
                account.nonce = nonce;
            }

            // balance
            if let Some(balance) = changes.balance.take_if_modified() {
                tracing::trace!(%balance, "saving balance");
                account.balance = balance
            }

            // bytecode
            if completed {
                if let Some(Some(bytecode)) = changes.bytecode.take_if_modified() {
                    tracing::trace!(bytecode_len = %bytecode.len(), "saving bytecode");
                    account.bytecode = Some(bytecode);
                }
            }

            // storage
            if completed {
                for (slot_index, mut slot) in changes.slots {
                    if let Some(slot) = slot.take_if_modified() {
                        tracing::trace!(%slot, "saving slot");
                        account_slots.insert(slot_index, slot);
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
