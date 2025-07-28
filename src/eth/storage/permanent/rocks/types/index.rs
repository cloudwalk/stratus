use std::fmt::Debug;

use crate::eth::primitives::Index;
use crate::eth::storage::permanent::rocks::cf_versions::SerializeDeserializeWithContext;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Copy, Hash, fake::Dummy)]
pub struct IndexRocksdb(pub(self) u32);

impl IndexRocksdb {
    pub fn as_usize(&self) -> usize {
        self.0 as usize
    }
}

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

impl SerializeDeserializeWithContext for IndexRocksdb {}
