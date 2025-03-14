use serde::Deserialize;
use serde::Serialize;

use crate::eth::primitives::Address;
use crate::eth::primitives::ExecutionAccountChanges;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, fake::Dummy)]
// TODO: improve naming
// TODO: create rocksdb newtypes for Address and ExecutionAccountChanges
pub struct BlockChangesRocksdb(pub Vec<(Address, ExecutionAccountChanges)>);

impl From<Vec<(Address, ExecutionAccountChanges)>> for BlockChangesRocksdb {
    fn from(changes: Vec<(Address, ExecutionAccountChanges)>) -> Self {
        Self(changes)
    }
}

impl BlockChangesRocksdb {
    pub fn into_changes(self) -> Vec<(Address, ExecutionAccountChanges)> {
        self.0
    }
}
