use std::fmt::Debug;

use ethereum_types::H256;

use crate::eth::primitives::Hash;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct HashRocksdb(H256);

impl HashRocksdb {
    pub fn inner_value(&self) -> H256 {
        self.0
    }
}

impl From<Hash> for HashRocksdb {
    fn from(item: Hash) -> Self {
        HashRocksdb(item.0)
    }
}

impl From<HashRocksdb> for Hash {
    fn from(item: HashRocksdb) -> Self {
        Hash::new_from_h256(item.inner_value())
    }
}
