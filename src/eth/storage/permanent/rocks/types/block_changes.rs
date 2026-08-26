use std::collections::HashMap;

use serde_with::serde_as;

use crate::eth::executor::types::state::AccountChanges;
use crate::eth::executor::types::state::Change;
use crate::eth::executor::types::state::Final;
use crate::eth::storage::permanent::rocks::types::AddressRocksdb;
use crate::eth::storage::permanent::rocks::types::SlotIndexRocksdb;
use crate::eth::storage::permanent::rocks::types::SlotValueRocksdb;
use crate::eth::storage::permanent::rocks::types::bytecode::BytecodeRocksdb;
use crate::eth::storage::permanent::rocks::types::nonce::NonceRocksdb;
use crate::eth::storage::permanent::rocks::types::wei::WeiRocksdb;
use crate::ext::OptionExt;

#[derive(Debug, Clone, PartialEq, bincode::Encode, bincode::Decode, serde::Serialize, serde::Deserialize, Default)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct AccountChangesRocksdb {
    pub balance: Option<WeiRocksdb>,
    pub nonce: Option<NonceRocksdb>,
    pub bytecode: Option<Option<BytecodeRocksdb>>,
}

impl AccountChangesRocksdb {
    pub fn has_changes(&self) -> bool {
        self.balance.is_some() || self.nonce.is_some() || self.bytecode.is_some()
    }
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, bincode::Encode, bincode::Decode, serde::Serialize, serde::Deserialize, Default)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct BlockChangesRocksdb {
    pub account_changes: HashMap<AddressRocksdb, AccountChangesRocksdb, hash_hasher::HashBuildHasher>,
    #[serde_as(as = "Vec<(_, _)>")]
    pub slot_changes: HashMap<(AddressRocksdb, SlotIndexRocksdb), SlotValueRocksdb, hash_hasher::HashBuildHasher>,
}

impl BlockChangesRocksdb {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            account_changes: HashMap::with_capacity_and_hasher(capacity, hash_hasher::HashBuildHasher::default()),
            slot_changes: HashMap::default(),
        }
    }
}

impl From<()> for BlockChangesRocksdb {
    fn from(_: ()) -> Self {
        unimplemented!()
    }
}

impl From<&AccountChanges<Final>> for AccountChangesRocksdb {
    fn from(value: &AccountChanges<Final>) -> Self {
        Self {
            balance: value.balance.changed_ref().copied().map_into(),
            nonce: value.nonce.changed_ref().copied().map_into(),
            bytecode: value.bytecode.changed_ref().cloned().map(|opt| opt.map_into()),
        }
    }
}
