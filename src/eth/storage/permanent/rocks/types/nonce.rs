use std::fmt::Debug;

use crate::eth::primitives::Nonce;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;
use crate::gen_newtype_from;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct NonceRocksdb(u64);

gen_newtype_from!(self = NonceRocksdb, other = u64);

impl From<NonceRocksdb> for Nonce {
    fn from(value: NonceRocksdb) -> Self {
        value.0.into()
    }
}

impl From<Nonce> for NonceRocksdb {
    fn from(value: Nonce) -> Self {
        u64::from(value).into()
    }
}

impl NonceRocksdb {
    pub const ZERO: NonceRocksdb = NonceRocksdb(0u64);
}

impl SerializeDeserializeWithContext for NonceRocksdb {}
