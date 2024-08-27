use std::fmt::Debug;
use std::ops::Add;

use ethereum_types::U64;

use crate::eth::primitives::BlockNumber;
use crate::gen_newtype_from;

#[derive(Debug, derive_more::Display, Clone, Copy, Default, Eq, PartialEq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub struct BlockNumberRocksdb(U64);

gen_newtype_from!(self = BlockNumberRocksdb, other = u8, u16, u32, u64, U64, usize, i32, i64);
impl BlockNumberRocksdb {
    pub fn inner_value(&self) -> U64 {
        self.0
    }
}

impl From<BlockNumber> for BlockNumberRocksdb {
    fn from(item: BlockNumber) -> Self {
        BlockNumberRocksdb(item.0)
    }
}

impl From<BlockNumberRocksdb> for BlockNumber {
    fn from(item: BlockNumberRocksdb) -> Self {
        BlockNumber::from(item.inner_value())
    }
}

impl From<BlockNumberRocksdb> for u64 {
    fn from(value: BlockNumberRocksdb) -> Self {
        value.0.as_u64()
    }
}

impl Add<u64> for BlockNumberRocksdb {
    type Output = Self;

    fn add(self, other: u64) -> Self {
        BlockNumberRocksdb(self.0 + other)
    }
}
