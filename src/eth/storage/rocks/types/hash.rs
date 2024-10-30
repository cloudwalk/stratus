use std::fmt::{Debug, Display};

use crate::eth::primitives::Hash;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct HashRocksdb([u8; 32]);

impl Display for HashRocksdb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", const_hex::encode_prefixed(self.0))
    }
}

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
