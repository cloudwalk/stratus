use display_json::DebugAsJson;
use rocksdb::WriteBatch;

use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;

#[derive(DebugAsJson, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct ExternalReplicationLog {
    pub block_number: BlockNumber,
    pub log_data: Bytes,
}

impl ExternalReplicationLog {
    pub fn new(block_number: BlockNumber, log_data: Bytes) -> Self {
        Self { block_number, log_data }
    }

    pub fn to_write_batch(&self) -> WriteBatch {
        WriteBatch::from_data(self.log_data.as_ref())
    }
}

impl From<(BlockNumber, Bytes)> for ExternalReplicationLog {
    fn from(value: (BlockNumber, Bytes)) -> Self {
        Self {
            block_number: value.0,
            log_data: value.1,
        }
    }
}

impl From<ExternalReplicationLog> for (BlockNumber, WriteBatch) {
    fn from(value: ExternalReplicationLog) -> Self {
        (value.block_number, value.to_write_batch())
    }
}
