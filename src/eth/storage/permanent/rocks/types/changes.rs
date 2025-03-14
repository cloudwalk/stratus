use serde::Deserialize;
use serde::Serialize;

use crate::eth::primitives::Address;
use crate::eth::primitives::ExecutionAccountChanges;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, fake::Dummy)]
// TODO: improve naming
// TODO: create rocksdb newtypes for Address and ExecutionAccountChanges
pub struct BlockChangesRocksdb(pub Vec<Vec<(Address, ExecutionAccountChanges)>>);

impl From<Vec<Vec<(Address, ExecutionAccountChanges)>>> for BlockChangesRocksdb {
    fn from(changes: Vec<Vec<(Address, ExecutionAccountChanges)>>) -> Self {
        Self(changes)
    }
}

impl BlockChangesRocksdb {
    pub fn into_changes(self) -> Vec<Vec<(Address, ExecutionAccountChanges)>> {
        self.0
    }
}
