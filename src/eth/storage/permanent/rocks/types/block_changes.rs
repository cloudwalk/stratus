use std::collections::BTreeMap;

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
    pub bytecode: Option<BytecodeRocksdb>,
    pub slot_changes: BTreeMap<SlotIndexRocksdb, SlotValueRocksdb>,
}

impl AccountChangesRocksdb {
    pub fn has_changes(&self) -> bool {
        !self.slot_changes.is_empty() || self.balance.is_some() || self.nonce.is_some() || self.bytecode.is_some()
    }
}

impl From<BlockChangesRocksdb> for ExecutionChanges {
    fn from(value: BlockChangesRocksdb) -> Self {
        let mut changes = ExecutionChanges::default();
        for (address, acc_changes) in value.account_changes {
            let addr = address.into();
            changes.accounts.insert(
                addr,
                ExecutionAccountChanges {
                    nonce: acc_changes.nonce.map_into(),
                    balance: acc_changes.balance.map_into(),
                    bytecode: Some(acc_changes.bytecode.map_into()),
                },
            );
            changes
                .slots
                .extend(acc_changes.slot_changes.into_iter().map(|(idx, val)| ((addr, idx.into()), val.into())));
        }
        changes
    }
}

#[derive(Debug, Clone, PartialEq, bincode::Encode, bincode::Decode, fake::Dummy, serde::Serialize, serde::Deserialize, Default)]
pub struct BlockChangesRocksdb {
    pub account_changes: BTreeMap<AddressRocksdb, AccountChangesRocksdb>,
}

impl From<()> for BlockChangesRocksdb {
    fn from(_: ()) -> Self {
        unimplemented!()
    }
}
