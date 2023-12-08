use std::fmt::Display;

use ethereum_types::U256;
use revm::primitives::StorageSlot;
use revm::primitives::U256 as RevmU256;

use crate::derive_newtype_from;

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

#[derive(Debug, Clone, Default, Eq, PartialEq, Hash)]
pub struct SlotIndex(U256);

impl Display for SlotIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#x}", self.0)
    }
}

derive_newtype_from!(self = SlotIndex, other = U256);

impl From<RevmU256> for SlotIndex {
    fn from(value: RevmU256) -> Self {
        Self(value.to_be_bytes().into())
    }
}

// -----------------------------------------------------------------------------
// SlotValue
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct SlotValue(U256);

impl Display for SlotValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#x}", self.0)
    }
}

derive_newtype_from!(self = SlotValue, other = U256);

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
