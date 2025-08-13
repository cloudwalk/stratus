use crate::eth::storage::permanent::rocks::types::{SlotIndexRocksdb, SlotValueRocksdb, bytecode::BytecodeRocksdb, nonce::NonceRocksdb, wei::WeiRocksdb};

#[derive(Debug, Clone, PartialEq, bincode::Encode, bincode::Decode, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct AccountChangesRocksdb {
    pub balance: Option<WeiRocksdb>,
    pub nonce: Option<NonceRocksdb>,
    pub bytecode: Option<BytecodeRocksdb>,
}

#[derive(Debug, Clone, PartialEq, bincode::Encode, bincode::Decode, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct BlockChangesRocksdb {
    account_changes: Vec<AccountChangesRocksdb>,
    slot_changes: Vec<(SlotIndexRocksdb, SlotValueRocksdb)>,
}

impl From<()> for BlockChangesRocksdb {
    fn from(_: ()) -> Self {
        unimplemented!()
    }
}
