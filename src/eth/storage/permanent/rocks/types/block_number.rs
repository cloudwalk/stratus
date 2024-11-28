use std::fmt::Debug;
use std::ops::Add;

use crate::eth::primitives::BlockNumber;
use crate::gen_newtype_from;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Ord, PartialOrd, Hash, derive_more::Display, fake::Dummy)]
pub struct BlockNumberRocksdb(pub u64);

gen_newtype_from!(self = BlockNumberRocksdb, other = u8, u16, u32, u64);

impl From<BlockNumber> for BlockNumberRocksdb {
    fn from(item: BlockNumber) -> Self {
        item.0.as_u64().into()
    }
}

impl From<BlockNumberRocksdb> for BlockNumber {
    fn from(item: BlockNumberRocksdb) -> Self {
        item.0.into()
    }
}

impl From<BlockNumberRocksdb> for u64 {
    fn from(value: BlockNumberRocksdb) -> Self {
        value.0
    }
}

impl serde::Serialize for BlockNumberRocksdb {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.to_be().serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for BlockNumberRocksdb {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        u64::deserialize(deserializer).map(|v| Self(u64::from_be(v)))
    }
}

impl Add<u64> for BlockNumberRocksdb {
    type Output = Self;

    fn add(self, other: u64) -> Self {
        BlockNumberRocksdb(self.0 + other)
    }
}
