use std::fmt::Debug;

use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;

#[derive(Clone, Debug, Copy, Default, PartialEq, Eq, bincode::Encode, bincode::Decode, fake::Dummy, serde::Deserialize, serde::Serialize)]
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

#[derive(Clone, Debug, Copy, Default, Hash, PartialEq, Eq, PartialOrd, Ord, bincode::Encode, bincode::Decode, fake::Dummy)]
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

impl serde::Serialize for SlotIndexRocksdb {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let slot_index: SlotIndex = (*self).into();
        slot_index.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for SlotIndexRocksdb {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let slot_index = SlotIndex::deserialize(deserializer)?;
        Ok(slot_index.into())
    }
}

impl SerializeDeserializeWithContext for SlotValueRocksdb {}
impl SerializeDeserializeWithContext for SlotIndexRocksdb {}
