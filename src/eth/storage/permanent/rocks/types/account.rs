use std::fmt::Debug;

use super::address::AddressRocksdb;
use super::bytecode::BytecodeRocksdb;
use super::nonce::NonceRocksdb;
use super::wei::WeiRocksdb;
use crate::eth::executor::types::state::AccountChanges;
use crate::eth::executor::types::state::Change;
use crate::eth::executor::types::state::Final;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;
use crate::eth::types::Account;
use crate::eth::types::Address;
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

    pub fn update(mut self, other: AccountChanges<Final>) -> Self {
        if other.balance.is_changed() {
            self.balance = other.balance.take_value().into();
        }

        if other.nonce.is_changed() {
            self.nonce = other.nonce.take_value().into();
        }

        if other.bytecode.is_changed() {
            self.bytecode = other.bytecode.take_value().map_into();
        }

        self
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
