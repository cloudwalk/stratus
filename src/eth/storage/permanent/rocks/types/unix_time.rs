use std::fmt::Debug;

use crate::eth::primitives::UnixTime;
use crate::gen_newtype_from;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct UnixTimeRocksdb(pub u64);

gen_newtype_from!(self = UnixTimeRocksdb, other = u64);

impl From<UnixTime> for UnixTimeRocksdb {
    fn from(value: UnixTime) -> Self {
        Self(*value)
    }
}

impl From<UnixTimeRocksdb> for UnixTime {
    fn from(value: UnixTimeRocksdb) -> Self {
        value.0.into()
    }
}
