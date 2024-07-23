

use crate::eth::primitives::ExecutionResult;

pub enum ExecutionResultRocksdb {
    Success,
    Reverted,
    Halted { reason: String },
}

impl From<ExecutionResult> for ExecutionResultRocksdb {
    fn from(item: ExecutionResult) -> Self {
        match item {
            ExecutionResult::Success => ExecutionResultRocksdb::Success,
            ExecutionResult::Reverted => ExecutionResultRocksdb::Reverted,
            ExecutionResult::Halted { reason } => ExecutionResultRocksdb::Halted { reason },
        }
    }
}

impl From<ExecutionResultRocksdb> for ExecutionResult {
    fn from(item: ExecutionResultRocksdb) -> Self {
        match item {
            ExecutionResultRocksdb::Success => ExecutionResult::Success,
            ExecutionResultRocksdb::Reverted => ExecutionResult::Reverted,
            ExecutionResultRocksdb::Halted { reason } => ExecutionResult::Halted { reason },
        }
    }
}
