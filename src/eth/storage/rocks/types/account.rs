use std::fmt::Debug;

use revm::primitives::KECCAK_EMPTY;

use super::address::AddressRocksdb;
use super::bytes::BytesRocksdb;
use super::nonce::NonceRocksdb;
use super::wei::WeiRocksdb;
use crate::eth::primitives::Account;
use crate::eth::primitives::Address;
use crate::ext::OptionExt;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct AccountRocksdb {
    pub balance: WeiRocksdb,
    pub nonce: NonceRocksdb,
    pub bytecode: Option<BytesRocksdb>,
}

impl AccountRocksdb {
    pub fn to_account(&self, address: &Address) -> Account {
        Account {
            address: *address,
            nonce: self.nonce.clone().into(),
            balance: self.balance.clone().into(),
            bytecode: self.bytecode.clone().map_into(),
            code_hash: KECCAK_EMPTY.into(),
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

impl Default for AccountRocksdb {
    fn default() -> Self {
        Self {
            balance: WeiRocksdb::ZERO,
            nonce: NonceRocksdb::ZERO,
            bytecode: None,
        }
    }
}
