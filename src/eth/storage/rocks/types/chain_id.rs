use std::fmt::Debug;

use crate::eth::primitives::ChainId;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChainIdRocksdb(pub u64);

impl From<ChainId> for ChainIdRocksdb {
    fn from(value: ChainId) -> Self {
        ChainIdRocksdb(value.0.as_u64())
    }
}

impl From<ChainIdRocksdb> for ChainId {
    fn from(value: ChainIdRocksdb) -> Self {
        value.0.into()
    }
}
