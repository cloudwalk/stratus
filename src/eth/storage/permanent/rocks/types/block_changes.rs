use std::collections::BTreeMap;

use crate::alias::RevmBytecode;
use crate::eth::primitives::ExecutionAccountChanges;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Wei;
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
                        nonce: changes.nonce.map(Nonce::from).into(),
                        balance: changes.balance.map(Wei::from).into(),
                        bytecode: changes.bytecode.map(|inner| Some(RevmBytecode::from(inner))).into(),
                        slots: changes
                            .slot_changes
                            .into_iter()
                            .map(|(idx, value)| (idx.into(), value.into()))
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
