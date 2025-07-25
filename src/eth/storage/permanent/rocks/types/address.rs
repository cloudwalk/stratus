use std::fmt::Debug;

use crate::eth::primitives::Address;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, bincode::Encode, bincode::Decode, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct AddressRocksdb(pub [u8; 20]);

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
