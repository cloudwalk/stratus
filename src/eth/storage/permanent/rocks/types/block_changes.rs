use std::collections::HashMap;

use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::storage::permanent::rocks::types::AddressRocksdb;
use crate::eth::storage::permanent::rocks::types::SlotIndexRocksdb;
use crate::eth::storage::permanent::rocks::types::SlotValueRocksdb;
use crate::eth::storage::permanent::rocks::types::bytecode::BytecodeRocksdb;
use crate::eth::storage::permanent::rocks::types::nonce::NonceRocksdb;
use crate::eth::storage::permanent::rocks::types::wei::WeiRocksdb;
use crate::ext::OptionExt;

#[derive(Debug, Clone, PartialEq, bincode::Encode, bincode::Decode, fake::Dummy, serde::Serialize, serde::Deserialize, Default)]
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

#[derive(Debug, Clone, PartialEq, bincode::Encode, bincode::Decode, fake::Dummy, serde::Serialize, serde::Deserialize, Default)]
pub struct BlockChangesRocksdb {
    pub account_changes: HashMap<AddressRocksdb, AccountChangesRocksdb>,
    pub slot_changes: HashMap<(AddressRocksdb, SlotIndexRocksdb), SlotValueRocksdb>,
}

impl BlockChangesRocksdb {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            account_changes: HashMap::with_capacity(capacity),
            slot_changes: HashMap::default(),
        }
    }

    pub fn to_incomplete_execution_changes(self) -> ExecutionChanges {
        let accounts = self
            .account_changes
            .into_iter()
            .map(|(address, changes)| {
                (
                    address.into(),
                    ExecutionAccountChanges {
                        nonce: changes.nonce.into(),
                        balance: changes.balance.into(),
                        bytecode: changes.bytecode.map(|inner| inner.map_into()).into(),
                    },
                )
            })
            .collect();
        let slots = self
            .slot_changes
            .into_iter()
            .map(|((addr, idx), value)| ((addr.into(), idx.into()), value.into()))
            .collect();
        ExecutionChanges { accounts, slots }
    }
}

impl From<()> for BlockChangesRocksdb {
    fn from(_: ()) -> Self {
        unimplemented!()
    }
}
