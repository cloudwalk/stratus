use std::fmt::Debug;

use crate::eth::primitives::Index;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, derive_more::Add, Copy, Hash, fake::Dummy)]
pub struct IndexRocksdb(pub(self) u32);

impl From<Index> for IndexRocksdb {
    fn from(item: Index) -> Self {
        IndexRocksdb(item.0 as u32)
    }
}

impl From<IndexRocksdb> for Index {
    fn from(item: IndexRocksdb) -> Self {
        Index::new(item.0 as u64)
    }
}
