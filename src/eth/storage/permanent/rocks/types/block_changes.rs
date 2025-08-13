use crate::eth::storage::permanent::rocks::types::{
    AddressRocksdb, SlotIndexRocksdb, SlotValueRocksdb, bytecode::BytecodeRocksdb, nonce::NonceRocksdb, wei::WeiRocksdb,
};

#[derive(Debug, Clone, PartialEq, bincode::Encode, bincode::Decode, fake::Dummy, serde::Serialize, serde::Deserialize, Default)]
pub struct AccountChangesRocksdb {
    pub address: AddressRocksdb,
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
    pub account_changes: Vec<AccountChangesRocksdb>,
    pub slot_changes: Vec<(AddressRocksdb, SlotIndexRocksdb, SlotValueRocksdb)>,
}

impl From<()> for BlockChangesRocksdb {
    fn from(_: ()) -> Self {
        unimplemented!()
    }
}
