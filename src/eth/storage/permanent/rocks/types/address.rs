use std::fmt::Debug;

use crate::eth::primitives::Address;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, fake::Dummy)]
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

impl SerializeDeserializeWithContext for AddressRocksdb {}
