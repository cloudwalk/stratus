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
    pub bytecode: Option<Bytes>,
    pub slots: HashMap<SlotIndex, InMemoryHistory<Slot>>,
}

impl InMemoryAccount {
    /// Creates a new account.
    pub fn new(address: Address) -> Self {
        Self::new_with_balance(address, Wei::ZERO)
    }

    /// Creates a new account with initial balanec.
    pub fn new_with_balance(address: Address, balance: Wei) -> Self {
        Self {
            address,
            balance: InMemoryHistory::new(BlockNumber::ZERO, balance),
            nonce: InMemoryHistory::new(BlockNumber::ZERO, Nonce::ZERO),
            bytecode: None,
            slots: Default::default(),
        }
    }

    /// Sets a new balance for the account tracking the history change.
    pub fn set_balance(&mut self, block_number: BlockNumber, balance: Wei) {
        self.balance.push(block_number, balance);
    }

    /// Sets a new nonce for the accoun tracking the history change.
    pub fn set_nonce(&mut self, block_number: BlockNumber, nonce: Nonce) {
        self.nonce.push(block_number, nonce);
    }

    /// Sets the account bytecode. Does not keep track because bytecode is set only during account creation.
    pub fn set_bytecode(&mut self, bytecode: Bytes) {
        self.bytecode = Some(bytecode);
    }
}
