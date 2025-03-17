use serde::Deserialize;
use serde::Serialize;

use super::address::AddressRocksdb;
use crate::eth::primitives::ExecutionAccountChanges;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, fake::Dummy)]
// TODO: improve naming
// TODO: create rocksdb newtypes for Address and ExecutionAccountChanges
pub struct BlockChangesRocksdb(pub Vec<Vec<(AddressRocksdb, ExecutionAccountChanges)>>);

impl From<Vec<Vec<(AddressRocksdb, ExecutionAccountChanges)>>> for BlockChangesRocksdb {
    fn from(changes: Vec<Vec<(AddressRocksdb, ExecutionAccountChanges)>>) -> Self {
        Self(changes)
    }
}

impl BlockChangesRocksdb {
    pub fn into_changes(self) -> Vec<Vec<(AddressRocksdb, ExecutionAccountChanges)>> {
        self.0
    }
}
