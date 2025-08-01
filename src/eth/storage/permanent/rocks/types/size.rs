use std::fmt::Debug;

use crate::eth::primitives::Size;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;
use crate::gen_newtype_from;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
#[serde(transparent)]
pub struct SizeRocksdb(u64);

gen_newtype_from!(self = SizeRocksdb, other = u64);

impl From<Size> for SizeRocksdb {
    fn from(value: Size) -> Self {
        u64::from(value).into()
    }
}

impl From<SizeRocksdb> for Size {
    fn from(value: SizeRocksdb) -> Self {
        value.0.into()
    }
}

impl SerializeDeserializeWithContext for SizeRocksdb {}
