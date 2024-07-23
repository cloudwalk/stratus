use std::fmt::Debug;

use ethereum_types::U64;

use crate::eth::primitives::Size;
use crate::gen_newtype_from;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct SizeRocksdb(U64);

gen_newtype_from!(self = SizeRocksdb, other = U64, u64);

impl From<Size> for SizeRocksdb {
    fn from(value: Size) -> Self {
        u64::from(value).into()
    }
}

impl From<SizeRocksdb> for Size {
    fn from(value: SizeRocksdb) -> Self {
        value.0.as_u64().into()
    }
}
