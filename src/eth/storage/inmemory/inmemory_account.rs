use std::collections::HashMap;

use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::Wei;
use crate::eth::storage::inmemory::InMemoryHistory;

#[derive(Debug)]
pub struct InMemoryAccount {
    pub address: Address,
    pub balance: InMemoryHistory<Wei>,
    pub nonce: InMemoryHistory<Nonce>,
    pub bytecode: InMemoryHistory<Option<Bytes>>,
    pub slots: HashMap<SlotIndex, InMemoryHistory<Slot>>,
}

impl InMemoryAccount {
    /// Creates a new account.
    pub fn new(address: Address) -> Self {
        Self::new_with_balance(address, Wei::ZERO)
    }

    /// Creates a new account with initial balance.
    pub fn new_with_balance(address: Address, balance: Wei) -> Self {
        Self {
            address,
            balance: InMemoryHistory::new_at_zero(balance),
            nonce: InMemoryHistory::new_at_zero(Nonce::ZERO),
            bytecode: InMemoryHistory::new_at_zero(None),
            slots: Default::default(),
        }
    }

    /// Resets all account changes to the specified block number.
    pub fn reset(&mut self, block_number: BlockNumber) {
        // SAFETY: ok to unwrap because all historical values starts at block 0
        self.balance = self.balance.reset(block_number).expect("never empty");
        self.nonce = self.nonce.reset(block_number).expect("never empty");
        self.bytecode = self.bytecode.reset(block_number).expect("never empty");

        // SAFETY: not ok to unwrap because slot value does not start at block 0
        let mut new_slots = HashMap::with_capacity(self.slots.len());
        for (slot_index, slot_history) in self.slots.iter() {
            if let Some(new_slot_history) = slot_history.reset(block_number) {
                new_slots.insert(slot_index.clone(), new_slot_history);
            }
        }
        self.slots = new_slots;
    }

    /// Sets a new balance for the account tracking the history change.
    pub fn set_balance(&mut self, block_number: BlockNumber, balance: Wei) {
        self.balance.push(block_number, balance);
    }

    /// Sets a new nonce for the account tracking the history change.
    pub fn set_nonce(&mut self, block_number: BlockNumber, nonce: Nonce) {
        self.nonce.push(block_number, nonce);
    }

    /// Sets the account bytecode. Does not keep track because bytecode is set only during account creation.
    pub fn set_bytecode(&mut self, block_number: BlockNumber, bytecode: Bytes) {
        self.bytecode.push(block_number, Some(bytecode));
    }
}
