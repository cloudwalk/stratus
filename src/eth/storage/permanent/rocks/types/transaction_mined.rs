use std::fmt::Debug;

use super::block_number::BlockNumberRocksdb;
use super::execution::ExecutionRocksdb;
use super::hash::HashRocksdb;
use super::index::IndexRocksdb;
use super::log_mined::LogMinedRocksdb;
use super::transaction_input::TransactionInputRocksdb;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct TransactionMinedRocksdb {
    pub input: TransactionInputRocksdb,
    pub execution: ExecutionRocksdb,
    pub logs: Vec<LogMinedRocksdb>,
    pub transaction_index: IndexRocksdb,
}

impl From<TransactionMined> for TransactionMinedRocksdb {
    fn from(item: TransactionMined) -> Self {
        Self {
            input: item.input.into(),
            execution: item.execution.into(),
            logs: item.logs.into_iter().map(LogMinedRocksdb::from).collect(),
            transaction_index: IndexRocksdb::from(item.transaction_index),
        }
    }
}

impl TransactionMined {
    pub fn from_rocks_primitives(other: TransactionMinedRocksdb, block_number: BlockNumberRocksdb, block_hash: HashRocksdb) -> Self {
        let logs = other
            .logs
            .into_iter()
            .map(|log| {
                LogMined::from_rocks_primitives(
                    log.log,
                    block_number,
                    block_hash,
                    other.transaction_index.as_usize(),
                    other.input.hash,
                    log.index,
                )
            })
            .collect();
        Self {
            block_number: block_number.into(),
            block_hash: block_hash.into(),
            input: other.input.into(),
            execution: other.execution.into(),
            transaction_index: other.transaction_index.into(),
            logs,
        }
    }
}

impl SerializeDeserializeWithContext for TransactionMinedRocksdb {}
