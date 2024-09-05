use std::fmt::Debug;

use ethereum_types::U256;

use crate::eth::primitives::Wei;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct WeiRocksdb([u64; 4]);

impl From<U256> for WeiRocksdb {
    fn from(value: U256) -> Self {
        Self(value.0)
    }
}

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
    pub const ZERO: WeiRocksdb = WeiRocksdb([0; 4]);
    pub const ONE: WeiRocksdb = WeiRocksdb([1, 0, 0, 0]);
}
