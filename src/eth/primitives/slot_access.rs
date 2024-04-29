use std::fmt::Display;

use crate::eth::primitives::SlotIndex;

/// How a slot is accessed.
#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum SlotAccess {
    /// Slot index will be accessed statically without any hashing.
    Static(SlotIndex),

    /// Index will be hashed according to mapping hash algorithm.
    Mapping(SlotIndex),

    /// Index will be hashed according to array hashing algorithm.
    Array(SlotIndex),
}

impl Display for SlotAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SlotAccess::Static(index) => write!(f, "Static/{}", index),
            SlotAccess::Mapping(index) => write!(f, "Mapping/{}", index),
            SlotAccess::Array(index) => write!(f, "Array/{}", index),
        }
    }
}
