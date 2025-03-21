use rocksdb::WriteBatch;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, fake::Dummy)]
pub struct WriteBatchRocksdb {
    pub data: Vec<u8>,
}

impl From<WriteBatch> for WriteBatchRocksdb {
    fn from(batch: WriteBatch) -> Self {
        Self { data: batch.data().to_vec() }
    }
}

impl WriteBatchRocksdb {
    pub fn to_write_batch(&self) -> WriteBatch {
        WriteBatch::from_data(&self.data)
    }
}
