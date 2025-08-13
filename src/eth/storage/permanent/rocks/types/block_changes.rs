use crate::{
    alias::RevmBytecode,
    eth::{
        primitives::{ExecutionAccountChanges, ExecutionChanges, ExecutionValueChange, Nonce, Slot, Wei},
        storage::permanent::rocks::types::{
            AddressRocksdb, SlotIndexRocksdb, SlotValueRocksdb, bytecode::BytecodeRocksdb, nonce::NonceRocksdb, wei::WeiRocksdb,
        },
    },
};
use std::collections::BTreeMap;

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
        value
            .account_changes
            .into_iter()
            .map(|(address, changes)| {
                (
                    address.into(),
                    ExecutionAccountChanges {
                        new_account: false,
                        address: address.into(),
                        nonce: changes.nonce.map(|inner| Nonce::from(inner)).into(),
                        balance: changes.balance.map(|inner| Wei::from(inner)).into(),
                        bytecode: changes.bytecode.map(|inner| Some(RevmBytecode::from(inner))).into(),
                        code_hash: Default::default(),
                        slots: changes
                            .slot_changes
                            .into_iter()
                            .map(|(idx, value)| (idx.into(), ExecutionValueChange::from_modified(Slot::new(idx.into(), value.into()))))
                            .collect(),
                    },
                )
            })
            .collect()
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
