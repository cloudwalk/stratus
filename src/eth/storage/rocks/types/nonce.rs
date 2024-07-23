use std::fmt::Debug;

use ethereum_types::U64;

use crate::eth::primitives::Nonce;
use crate::gen_newtype_from;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NonceRocksdb(U64);

gen_newtype_from!(self = NonceRocksdb, other = u64);

impl From<NonceRocksdb> for Nonce {
    fn from(value: NonceRocksdb) -> Self {
        value.0.as_u64().into()
    }
}

impl From<Nonce> for NonceRocksdb {
    fn from(value: Nonce) -> Self {
        u64::from(value).into()
    }
}

impl NonceRocksdb {
    pub const ZERO: NonceRocksdb = NonceRocksdb(U64::zero());
}
