use std::fmt::Debug;

use ethereum_types::H160;

use crate::eth::primitives::Address;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AddressRocksdb(H160);

impl AddressRocksdb {
    pub fn inner_value(&self) -> H160 {
        self.0
    }
}

impl From<Address> for AddressRocksdb {
    fn from(item: Address) -> Self {
        AddressRocksdb(item.0)
    }
}

impl From<AddressRocksdb> for Address {
    fn from(item: AddressRocksdb) -> Self {
        Address::new_from_h160(item.inner_value())
    }
}
