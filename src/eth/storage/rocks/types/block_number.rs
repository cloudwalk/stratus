use std::fmt::Debug;

use crate::eth::primitives::BlockNumber;
use crate::gen_newtype_from;

#[derive(Debug, derive_more::Display, Clone, Copy, Default, Eq, PartialEq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
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
