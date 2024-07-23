use std::fmt::Debug;

use ethereum_types::U64;

use crate::eth::primitives::ChainId;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChainIdRocksdb(U64);

impl ChainIdRocksdb {
    pub fn inner_value(&self) -> U64 {
        self.0
    }
}

impl From<ChainId> for ChainIdRocksdb {
    fn from(value: ChainId) -> Self {
        ChainIdRocksdb(value.0)
    }
}

impl From<ChainIdRocksdb> for ChainId {
    fn from(value: ChainIdRocksdb) -> Self {
        ChainId::new(value.inner_value())
    }
}
