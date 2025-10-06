use std::fmt::Debug;

use crate::eth::primitives::ChainId;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;
use crate::ext::RuintExt;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, bincode::Encode, bincode::Decode, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(fake::Dummy))]
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

impl SerializeDeserializeWithContext for ChainIdRocksdb {}
