use std::fmt::Debug;

use crate::eth::primitives::Gas;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;
use crate::gen_newtype_from;

#[derive(Debug, Clone, Default, PartialEq, Eq, bincode::Encode, bincode::Decode, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(fake::Dummy))]
#[serde(transparent)]
pub struct GasRocksdb(u64);

gen_newtype_from!(self = GasRocksdb, other = u64);

impl From<GasRocksdb> for Gas {
    fn from(value: GasRocksdb) -> Self {
        value.0.into()
    }
}

impl From<Gas> for GasRocksdb {
    fn from(value: Gas) -> Self {
        u64::from(value).into()
    }
}

impl SerializeDeserializeWithContext for GasRocksdb {}
