use std::fmt::Debug;

use alloy_primitives::U256;

use crate::eth::primitives::Wei;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct WeiRocksdb([u64; 4]);

impl From<U256> for WeiRocksdb {
    fn from(value: U256) -> Self {
        Self(value.into_limbs())
    }
}

impl From<u128> for WeiRocksdb {
    fn from(value: u128) -> Self {
        Self(U256::from(value).into_limbs())
    }
}

impl From<WeiRocksdb> for Wei {
    fn from(value: WeiRocksdb) -> Self {
        value.0.into()
    }
}

impl From<WeiRocksdb> for u128 {
    fn from(value: WeiRocksdb) -> Self {
        U256::from_limbs(value.0).to::<u128>()
    }
}

impl From<Wei> for WeiRocksdb {
    fn from(value: Wei) -> Self {
        value.0.into()
    }
}

impl WeiRocksdb {
    pub const ZERO: WeiRocksdb = WeiRocksdb([0; 4]);
    pub const ONE: WeiRocksdb = WeiRocksdb([1, 0, 0, 0]);
}

impl SerializeDeserializeWithContext for WeiRocksdb {}
