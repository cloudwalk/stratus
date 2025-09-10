use std::collections::HashMap;

use crate::eth::storage::permanent::rocks::types::AddressRocksdb;
use crate::eth::storage::permanent::rocks::types::SlotIndexRocksdb;
use crate::eth::storage::permanent::rocks::types::SlotValueRocksdb;
use crate::eth::storage::permanent::rocks::types::bytecode::BytecodeRocksdb;
use crate::eth::storage::permanent::rocks::types::nonce::NonceRocksdb;
use crate::eth::storage::permanent::rocks::types::wei::WeiRocksdb;

#[derive(Debug, Clone, PartialEq, bincode::Encode, bincode::Decode, fake::Dummy, serde::Serialize, serde::Deserialize, Default)]
pub struct AccountChangesRocksdb {
    pub balance: Option<WeiRocksdb>,
    pub nonce: Option<NonceRocksdb>,
    pub bytecode: Option<BytecodeRocksdb>,
}

impl AccountChangesRocksdb {
    pub fn has_changes(&self) -> bool {
        self.balance.is_some() || self.nonce.is_some() || self.bytecode.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, bincode::Encode, bincode::Decode, fake::Dummy, serde::Serialize, serde::Deserialize, Default)]
pub struct BlockChangesRocksdb {
    pub account_changes: HashMap<AddressRocksdb, AccountChangesRocksdb>,
    pub slot_changes: HashMap<(AddressRocksdb, SlotIndexRocksdb), SlotValueRocksdb>,
}

impl BlockChangesRocksdb {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            account_changes: HashMap::with_capacity(capacity),
            slot_changes: HashMap::default()
        }
    }
}

impl From<()> for BlockChangesRocksdb {
    fn from(_: ()) -> Self {
        unimplemented!()
    }
}
