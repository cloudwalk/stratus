use std::fmt::Debug;

use crate::eth::primitives::ChainId;
use crate::eth::storage::permanent::rocks::cf_versions::SerializeDeserializeWithContext;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
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
