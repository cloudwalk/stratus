use crate::eth::primitives::Address;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::Wei;
use crate::ext::not;

#[derive(Debug, Default)]
pub struct ExecutionConflicts {
    inner: Vec<ExecutionConflict>,
}

impl ExecutionConflicts {
    /// Adds a new nonce conflict to the list of tracked conflicts.
    pub fn add_nonce(&mut self, address: Address, expected: Nonce, actual: Nonce) {
        self.inner.push(ExecutionConflict::Nonce { address, expected, actual });
    }

    /// Adds a new balance conflict to the list of tracked conflicts.
    pub fn add_balance(&mut self, address: Address, expected: Wei, actual: Wei) {
        self.inner.push(ExecutionConflict::Balance { address, expected, actual });
    }

    /// Adds a new slot conflict to the list of tracked conflicts.
    pub fn add_slot(&mut self, address: Address, slot: SlotIndex, expected: SlotValue, actual: SlotValue) {
        self.inner.push(ExecutionConflict::Slot {
            address,
            slot,
            expected,
            actual,
        });
    }

    /// Indicates whether there are any tracked conflicts.
    pub fn any(&self) -> bool {
        not(self.inner.is_empty())
    }
}

#[derive(Debug, derive_new::new)]
pub enum ExecutionConflict {
    /// Account nonce mismatch.
    Nonce { address: Address, expected: Nonce, actual: Nonce },

    /// Account balance mismatch.
    Balance { address: Address, expected: Wei, actual: Wei },

    /// Slot value mismatch.
    Slot {
        address: Address,
        slot: SlotIndex,
        expected: SlotValue,
        actual: SlotValue,
    },
}
