use std::fmt::Debug;

use crate::eth::primitives::Index;

#[derive(Debug, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize, derive_more::Add, Copy, Hash)]
pub struct IndexRocksdb(u64);

impl IndexRocksdb {
    pub fn inner_value(&self) -> u64 {
        self.0
    }
}

impl From<Index> for IndexRocksdb {
    fn from(item: Index) -> Self {
        IndexRocksdb(item.0)
    }
}

impl From<IndexRocksdb> for Index {
    fn from(item: IndexRocksdb) -> Self {
        Index::new(item.inner_value())
    }
}
