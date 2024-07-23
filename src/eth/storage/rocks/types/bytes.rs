use std::fmt::Debug;
use std::fmt::Display;
use std::ops::Deref;

use crate::eth::primitives::Bytes;

#[derive(Clone, Default, Eq, PartialEq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct BytesRocksdb(pub Vec<u8>);

impl Deref for BytesRocksdb {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for BytesRocksdb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.len() <= 256 {
            write!(f, "{}", const_hex::encode_prefixed(&self.0))
        } else {
            write!(f, "too long")
        }
    }
}

impl Debug for BytesRocksdb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Bytes").field(&self.to_string()).finish()
    }
}

impl From<Bytes> for BytesRocksdb {
    fn from(value: Bytes) -> Self {
        Self(value.0)
    }
}

impl From<BytesRocksdb> for Bytes {
    fn from(value: BytesRocksdb) -> Self {
        Self(value.0)
    }
}
