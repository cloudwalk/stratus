use std::fmt::Debug;

use crate::eth::primitives::BlockNumber;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;
use crate::gen_newtype_from;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Ord, PartialOrd, Hash, derive_more::Display, fake::Dummy)]
pub struct BlockNumberRocksdb(pub u32);

gen_newtype_from!(self = BlockNumberRocksdb, other = u8, u16, u32);

impl From<BlockNumber> for BlockNumberRocksdb {
    fn from(item: BlockNumber) -> Self {
        Self(item.as_u32())
    }
}

impl From<BlockNumberRocksdb> for BlockNumber {
    fn from(item: BlockNumberRocksdb) -> Self {
        item.0.into()
    }
}

impl serde::Serialize for BlockNumberRocksdb {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.to_be().serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for BlockNumberRocksdb {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        u32::deserialize(deserializer).map(|v| Self(u32::from_be(v)))
    }
}

impl SerializeDeserializeWithContext for BlockNumberRocksdb {}
