use std::fmt::Debug;

use crate::eth::primitives::Address;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct AddressRocksdb([u8; 20]);

impl AddressRocksdb {
    pub fn inner_value(&self) -> [u8; 20] {
        self.0
    }
}

impl From<Address> for AddressRocksdb {
    fn from(item: Address) -> Self {
        AddressRocksdb(item.0.into())
    }
}

impl From<AddressRocksdb> for Address {
    fn from(item: AddressRocksdb) -> Self {
        item.0.into()
    }
}
