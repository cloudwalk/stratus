use display_json::DebugAsJson;

use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;

#[derive(DebugAsJson, Clone, Copy, Default, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct Slot {
    pub index: SlotIndex,
    pub value: SlotValue,
}

impl Slot {
    /// Creates a new slot with the given index and value.
    pub fn new(index: SlotIndex, value: SlotValue) -> Self {
        Self { index, value }
    }

    /// Creates a new slot with the given index and default zero value.
    pub fn new_empty(index: SlotIndex) -> Self {
        Self {
            index,
            value: SlotValue::default(),
        }
    }
}
