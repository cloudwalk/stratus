use std::fmt::Display;

use primitive_types::U256;
use revm::primitives::StorageSlot;
use revm::primitives::U256 as RevmU256;

#[derive(Debug, Clone, Default)]
pub struct Slot {
    pub index: SlotIndex,
    pub previous: SlotValue,
    pub present: SlotValue,
}

impl Display for Slot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}=({}->{})", self.index, self.previous, self.present)
    }
}

impl From<(RevmU256, StorageSlot)> for Slot {
    fn from((index, slot): (RevmU256, StorageSlot)) -> Self {
        Self {
            index: index.into(),
            previous: slot.original_value().into(),
            present: slot.present_value().into(),
        }
    }
}

// -----------------------------------------------------------------------------
// SlotIndex
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Eq, PartialEq, Hash, derive_more::From)]
pub struct SlotIndex(U256);

impl Display for SlotIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#x}", self.0)
    }
}

impl From<RevmU256> for SlotIndex {
    fn from(value: RevmU256) -> Self {
        Self(value.to_be_bytes().into())
    }
}

// -----------------------------------------------------------------------------
// SlotValue
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Default, derive_more::From)]
pub struct SlotValue(U256);

impl Display for SlotValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#x}", self.0)
    }
}

impl From<RevmU256> for SlotValue {
    fn from(value: RevmU256) -> Self {
        Self(value.to_be_bytes().into())
    }
}

impl From<SlotValue> for RevmU256 {
    fn from(value: SlotValue) -> Self {
        RevmU256::from_limbs(value.0 .0)
    }
}
