use std::fmt::Debug;
use std::fmt::Display;
use std::ops::Deref;

use revm::primitives::Bytes as RevmBytes;
use rocksdb::WriteBatch;

use crate::eth::primitives::Bytes;

#[derive(Clone, Default, PartialEq, Eq, bincode::Encode, bincode::Decode, fake::Dummy, serde::Serialize, serde::Deserialize)]
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

impl From<Vec<u8>> for BytesRocksdb {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

impl From<RevmBytes> for BytesRocksdb {
    fn from(value: RevmBytes) -> Self {
        value.to_vec().into()
    }
}

impl From<BytesRocksdb> for RevmBytes {
    fn from(value: BytesRocksdb) -> Self {
        value.to_vec().into()
    }
}

impl From<WriteBatch> for BytesRocksdb {
    fn from(batch: WriteBatch) -> Self {
        Self(batch.data().to_vec())
    }
}

impl BytesRocksdb {
    pub fn to_write_batch(&self) -> WriteBatch {
        WriteBatch::from_data(&self.0)
    }
}
