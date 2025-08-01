use display_json::DebugAsJson;
use rocksdb::WriteBatch;

use super::block_number::BlockNumberRocksdb;
use super::bytes::BytesRocksdb;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;

#[derive(DebugAsJson, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct ReplicationLogRocksdb {
    pub block_number: BlockNumberRocksdb,
    pub log_data: BytesRocksdb,
}

impl ReplicationLogRocksdb {
    pub fn new(block_number: BlockNumber, log_data: Bytes) -> Self {
        Self {
            block_number: BlockNumberRocksdb::from(block_number),
            log_data: BytesRocksdb::from(log_data),
        }
    }

    pub fn to_write_batch(&self) -> WriteBatch {
        self.log_data.to_write_batch()
    }

    pub fn data(&self) -> &[u8] {
        &self.log_data.0
    }
}

impl SerializeDeserializeWithContext for ReplicationLogRocksdb {}
