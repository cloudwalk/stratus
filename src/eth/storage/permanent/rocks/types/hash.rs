use std::fmt::Debug;

use crate::eth::primitives::Hash;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, bincode::Encode, bincode::Decode, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct HashRocksdb([u8; 32]);

impl From<Hash> for HashRocksdb {
    fn from(item: Hash) -> Self {
        HashRocksdb(item.0.into())
    }
}

impl From<HashRocksdb> for Hash {
    fn from(item: HashRocksdb) -> Self {
        item.0.into()
    }
}

impl SerializeDeserializeWithContext for HashRocksdb {}
