use display_json::DebugAsJson;
use nonempty::NonEmpty;

use super::BlockNumber;
use crate::eth::primitives::Address;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;
use crate::eth::primitives::Wei;

#[derive(DebugAsJson, serde::Serialize)]
pub struct ExecutionConflicts(pub NonEmpty<ExecutionConflict>);

#[derive(Debug, Default)]
pub struct ExecutionConflictsBuilder(Vec<ExecutionConflict>);

impl ExecutionConflictsBuilder {
    /// Adds a new nonce conflict to the list of tracked conflicts.
    pub fn add_nonce(&mut self, address: Address, expected: Nonce, actual: Nonce) {
        self.0.push(ExecutionConflict::Nonce { address, expected, actual });
    }

    /// Adds a new balance conflict to the list of tracked conflicts.
    pub fn add_balance(&mut self, address: Address, expected: Wei, actual: Wei) {
        self.0.push(ExecutionConflict::Balance { address, expected, actual });
    }

    /// Adds a new slot conflict to the list of tracked conflicts.
    pub fn add_slot(&mut self, address: Address, slot: SlotIndex, expected: SlotValue, actual: SlotValue) {
        self.0.push(ExecutionConflict::Slot {
            address,
            slot,
            expected,
            actual,
        });
    }

    /// Builds the list of tracked conflicts into a non-empty list of conflicts.
    pub fn build(self) -> Option<ExecutionConflicts> {
        NonEmpty::from_vec(self.0).map(ExecutionConflicts)
    }
}

#[derive(DebugAsJson, serde::Serialize)]
#[cfg_attr(test, derive(serde::Deserialize, fake::Dummy, PartialEq))]
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

    /// Transaction was executed in block A but the current pending block is B (A != B)
    BlockNumber { execution: BlockNumber, pending: BlockNumber },
}
