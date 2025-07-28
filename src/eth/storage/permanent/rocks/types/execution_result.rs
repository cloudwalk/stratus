use super::bytes::BytesRocksdb;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ExecutionResult;
use crate::eth::storage::permanent::rocks::cf_versions::SerializeDeserializeWithContext;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub enum ExecutionResultRocksdb {
    Success,
    Reverted,
    Halted { reason: String },
}

impl From<ExecutionResult> for ExecutionResultRocksdb {
    fn from(item: ExecutionResult) -> Self {
        match item {
            ExecutionResult::Success => ExecutionResultRocksdb::Success,
            ExecutionResult::Reverted { .. } => ExecutionResultRocksdb::Reverted,
            ExecutionResult::Halted { reason } => ExecutionResultRocksdb::Halted { reason },
        }
    }
}

pub struct ExecutionResultBuilder(pub (ExecutionResultRocksdb, BytesRocksdb));

impl ExecutionResultBuilder {
    pub fn build(self) -> (ExecutionResult, Bytes) {
        self.into()
    }
}

impl From<ExecutionResultBuilder> for (ExecutionResult, Bytes) {
    fn from(item: ExecutionResultBuilder) -> Self {
        let (result, out) = item.0;
        let output: Bytes = out.into();
        match result {
            ExecutionResultRocksdb::Success => (ExecutionResult::Success, output),
            ExecutionResultRocksdb::Reverted => (ExecutionResult::Reverted { reason: (&output).into() }, output),
            ExecutionResultRocksdb::Halted { reason } => (ExecutionResult::Halted { reason }, output),
        }
    }
}

impl SerializeDeserializeWithContext for ExecutionResultRocksdb {}
