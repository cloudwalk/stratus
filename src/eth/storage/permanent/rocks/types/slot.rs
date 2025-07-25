use std::fmt::Debug;

use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;

#[derive(Clone, Debug, Copy, Default, PartialEq, Eq, bincode::Encode, bincode::Decode, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct SlotValueRocksdb([u64; 4]);

impl From<SlotValue> for SlotValueRocksdb {
    fn from(item: SlotValue) -> Self {
        SlotValueRocksdb(item.0.into_limbs())
    }
}

impl From<SlotValueRocksdb> for SlotValue {
    fn from(item: SlotValueRocksdb) -> Self {
        SlotValue::from(item.0)
    }
}

#[derive(
    Clone, Debug, Copy, Default, Hash, PartialEq, Eq, PartialOrd, Ord, bincode::Encode, bincode::Decode, fake::Dummy, serde::Serialize, serde::Deserialize,
)]
pub struct SlotIndexRocksdb([u64; 4]);

impl From<SlotIndex> for SlotIndexRocksdb {
    fn from(item: SlotIndex) -> Self {
        SlotIndexRocksdb(item.0.into_limbs())
    }
}

impl From<SlotIndexRocksdb> for SlotIndex {
    fn from(item: SlotIndexRocksdb) -> Self {
        SlotIndex::from(item.0)
    }
}
