use std::fmt::Debug;

use super::block_number::BlockNumberRocksdb;
use super::execution::ExecutionRocksdb;
use super::hash::HashRocksdb;
use super::log_mined::LogMinedRocksdb;
use super::transaction_input::TransactionInputRocksdb;
use crate::eth::primitives::Index;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::TransactionMined;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct TransactionMinedRocksdb {
    pub input: TransactionInputRocksdb,
    pub execution: ExecutionRocksdb,
    pub logs: Vec<LogMinedRocksdb>,
}

impl From<TransactionMined> for TransactionMinedRocksdb {
    fn from(item: TransactionMined) -> Self {
        Self {
            input: item.input.into(),
            execution: item.execution.into(),
            logs: item.logs.into_iter().map(LogMinedRocksdb::from).collect(),
        }
    }
}

impl TransactionMined {
    pub fn from_rocks_primitives(other: TransactionMinedRocksdb, block_number: BlockNumberRocksdb, block_hash: HashRocksdb, tx_index: usize) -> Self {
        let logs = other
            .logs
            .into_iter()
            .enumerate()
            .map(|(log_index, log)| LogMined::from_rocks_primitives(log, block_number, block_hash, tx_index, other.input.hash, log_index))
            .collect();
        Self {
            block_number: block_number.into(),
            block_hash: block_hash.into(),
            input: other.input.into(),
            execution: other.execution.into(),
            logs,
            transaction_index: Index::from(tx_index as u64),
        }
    }
}
