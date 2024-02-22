use std::collections::HashMap;
use std::ops::Deref;
use std::ops::DerefMut;

use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Slot;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::Wei;
use crate::eth::storage::inmemory::InMemoryHistory;

pub trait InMemoryAccount {
    fn new(address: Address) -> Self;
    fn get_current_nonce(&self) -> &Nonce;
    fn get_current_balance(&self) -> &Wei;
    fn get_current_slot(&self, slot_index: &SlotIndex) -> Option<&Slot>;
    fn set_balance(&mut self, block_number: BlockNumber, balance: Wei);
    fn set_nonce(&mut self, block_number: BlockNumber, nonce: Nonce);
    fn set_bytecode(&mut self, block_number: BlockNumber, bytecode: Bytes);
    fn set_slot(&mut self, block_number: BlockNumber, slot: Slot);
}

#[derive(Debug)]
pub struct InMemoryAccountPermanent {
    pub address: Address,
    pub balance: InMemoryHistory<Wei>,
    pub nonce: InMemoryHistory<Nonce>,
    pub bytecode: InMemoryHistory<Option<Bytes>>,
    pub slots: HashMap<SlotIndex, InMemoryHistory<Slot>>,
}

impl InMemoryAccount for InMemoryAccountPermanent {
    /// Creates a new account.
    fn new(address: Address) -> Self {
        Self::new_with_balance(address, Wei::ZERO)
    }

    fn get_current_balance(&self) -> &Wei {
        self.balance.get_current_ref()
    }

    fn get_current_nonce(&self) -> &Nonce {
        self.nonce.get_current_ref()
    }

    fn get_current_slot(&self, slot_index: &SlotIndex) -> Option<&Slot> {
        self.slots.get(slot_index).map(|value| value.get_current_ref())
    }

    /// Sets a new balance for the account tracking the history change.
    fn set_balance(&mut self, block_number: BlockNumber, balance: Wei) {
        self.balance.push(block_number, balance);
    }

    /// Sets a new nonce for the account tracking the history change.
    fn set_nonce(&mut self, block_number: BlockNumber, nonce: Nonce) {
        self.nonce.push(block_number, nonce);
    }

    /// Sets the account bytecode. Does not keep track because bytecode is set only during account creation.
    fn set_bytecode(&mut self, block_number: BlockNumber, bytecode: Bytes) {
        self.bytecode.push(block_number, Some(bytecode));
    }

    fn set_slot(&mut self, block_number: BlockNumber, slot: Slot) {
        match self.slots.get_mut(&slot.index) {
            Some(slot_history) => {
                slot_history.push(block_number, slot);
            }
            None => {
                self.slots.insert(slot.index.clone(), InMemoryHistory::new(block_number, slot));
            }
        }
    }
}

impl InMemoryAccountPermanent {
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
    pub fn reset_at(&mut self, block_number: BlockNumber) {
        // SAFETY: ok to unwrap because all historical values starts at block 0
        self.balance = self.balance.reset_at(block_number).expect("never empty");
        self.nonce = self.nonce.reset_at(block_number).expect("never empty");
        self.bytecode = self.bytecode.reset_at(block_number).expect("never empty");

        // SAFETY: not ok to unwrap because slot value does not start at block 0
        let mut new_slots = HashMap::with_capacity(self.slots.len());
        for (slot_index, slot_history) in self.slots.iter() {
            if let Some(new_slot_history) = slot_history.reset_at(block_number) {
                new_slots.insert(slot_index.clone(), new_slot_history);
            }
        }
        self.slots = new_slots;
    }
}

#[derive(Debug)]
pub struct InMemoryAccountTemporary {
    pub account: Account,
    pub slots: HashMap<SlotIndex, Slot>,
}

impl InMemoryAccount for InMemoryAccountTemporary {
    /// Creates a new account.
    fn new(address: Address) -> Self {
        Self {
            account: Account {
                address,
                balance: Wei::ZERO,
                nonce: Nonce::ZERO,
                bytecode: None,
            },
            slots: Default::default(),
        }
    }

    fn get_current_balance(&self) -> &Wei {
        &self.balance
    }

    fn get_current_nonce(&self) -> &Nonce {
        &self.nonce
    }

    fn get_current_slot(&self, slot_index: &SlotIndex) -> Option<&Slot> {
        self.slots.get(slot_index)
    }

    fn set_balance(&mut self, _block_number: BlockNumber, balance: Wei) {
        self.balance = balance
    }

    fn set_bytecode(&mut self, _block_number: BlockNumber, bytecode: Bytes) {
        self.bytecode = Some(bytecode)
    }

    fn set_nonce(&mut self, _block_number: BlockNumber, nonce: Nonce) {
        self.nonce = nonce
    }

    fn set_slot(&mut self, _block_number: BlockNumber, slot: Slot) {
        self.slots.insert(slot.index.clone(), slot);
    }
}

impl Deref for InMemoryAccountTemporary {
    type Target = Account;
    fn deref(&self) -> &Self::Target {
        &self.account
    }
}

impl DerefMut for InMemoryAccountTemporary {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.account
    }
}
