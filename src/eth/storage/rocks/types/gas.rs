use std::fmt::Debug;

use ethereum_types::U64;

use crate::eth::primitives::Gas;
use crate::gen_newtype_from;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct GasRocksdb(U64);

gen_newtype_from!(self = GasRocksdb, other = u64);

impl From<GasRocksdb> for Gas {
    fn from(value: GasRocksdb) -> Self {
        value.0.as_u64().into()
    }
}

impl From<Gas> for GasRocksdb {
    fn from(value: Gas) -> Self {
        u64::from(value).into()
    }
}
