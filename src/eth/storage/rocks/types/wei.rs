use std::fmt::Debug;

use ethereum_types::U256;

use crate::eth::primitives::Wei;
use crate::gen_newtype_from;

#[derive(Debug, Clone, Default, Eq, PartialEq, derive_more::Add, derive_more::Sub, serde::Serialize, serde::Deserialize)]
pub struct WeiRocksdb(U256);

gen_newtype_from!(self = WeiRocksdb, other = U256);

impl From<WeiRocksdb> for Wei {
    fn from(value: WeiRocksdb) -> Self {
        value.0.into()
    }
}

impl From<Wei> for WeiRocksdb {
    fn from(value: Wei) -> Self {
        U256::from(value).into()
    }
}

impl WeiRocksdb {
    pub const ZERO: WeiRocksdb = WeiRocksdb(U256::zero());
    pub const ONE: WeiRocksdb = WeiRocksdb(U256::one());
}
