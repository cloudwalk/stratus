use std::fmt::Debug;

use super::address::AddressRocksdb;
use super::bytecode::BytecodeRocksdb;
use super::nonce::NonceRocksdb;
use super::wei::WeiRocksdb;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;
use crate::ext::OptionExt;

#[derive(Debug, Clone, Default, PartialEq, Eq, bincode::Encode, bincode::Decode, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct AccountRocksdb {
    pub balance: WeiRocksdb,
    pub nonce: NonceRocksdb,
    pub bytecode: Option<BytecodeRocksdb>,
}

impl AccountRocksdb {
    pub fn to_account(&self, address: Address) -> Account {
        Account {
            address,
            nonce: self.nonce.into(),
            balance: self.balance.into(),
            bytecode: self.bytecode.clone().map_into(),
        }
    }
}

impl From<Account> for (AddressRocksdb, AccountRocksdb) {
    fn from(value: Account) -> Self {
        (
            value.address.into(),
            AccountRocksdb {
                balance: value.balance.into(),
                nonce: value.nonce.into(),
                bytecode: value.bytecode.map_into(),
            },
        )
    }
}

impl From<Account> for AccountRocksdb {
    fn from(value: Account) -> Self {
        AccountRocksdb {
            balance: value.balance.into(),
            nonce: value.nonce.into(),
            bytecode: value.bytecode.map_into(),
        }
    }
}

impl SerializeDeserializeWithContext for AccountRocksdb {}
