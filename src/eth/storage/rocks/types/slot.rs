use std::fmt::Debug;

use ethereum_types::U256;

use crate::eth::primitives::SlotIndex;
use crate::eth::primitives::SlotValue;
use crate::gen_newtype_from;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SlotValueRocksdb(U256);

impl SlotValueRocksdb {
    pub fn inner_value(&self) -> U256 {
        self.0
    }
}

impl From<SlotValue> for SlotValueRocksdb {
    fn from(item: SlotValue) -> Self {
        SlotValueRocksdb(item.as_u256())
    }
}

impl From<SlotValueRocksdb> for SlotValue {
    fn from(item: SlotValueRocksdb) -> Self {
        SlotValue::from(item.inner_value())
    }
}

#[derive(Clone, Copy, Default, Hash, Eq, PartialEq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct SlotIndexRocksdb(U256);

gen_newtype_from!(self = SlotIndexRocksdb, other = u64);

impl SlotIndexRocksdb {
    pub fn inner_value(&self) -> U256 {
        self.0
    }
}

impl From<SlotIndex> for SlotIndexRocksdb {
    fn from(item: SlotIndex) -> Self {
        SlotIndexRocksdb(item.as_u256())
    }
}

impl From<SlotIndexRocksdb> for SlotIndex {
    fn from(item: SlotIndexRocksdb) -> Self {
        SlotIndex::from(item.inner_value())
    }
}
