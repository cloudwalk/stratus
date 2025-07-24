use std::fmt::Debug;

use crate::eth::primitives::ChainId;
use crate::ext::RuintExt;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, bincode::Encode, bincode::Decode, fake::Dummy, serde::Serialize, serde::Deserialize)]
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
