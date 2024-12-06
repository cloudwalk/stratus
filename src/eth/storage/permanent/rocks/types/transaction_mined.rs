use std::fmt::Debug;

use super::block_number::BlockNumberRocksdb;
use super::execution::ExecutionRocksdb;
use super::hash::HashRocksdb;
use super::index::IndexRocksdb;
use super::log_mined::LogMinedRocksdb;
use super::transaction_input::TransactionInputRocksdb;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::TransactionMined;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct TransactionMinedRocksdb {
    pub input: TransactionInputRocksdb,
    pub execution: ExecutionRocksdb,
    pub logs: Vec<LogMinedRocksdb>,
    pub transaction_index: IndexRocksdb,
    pub block_number: BlockNumberRocksdb,
    pub block_hash: HashRocksdb,
}

impl From<TransactionMined> for TransactionMinedRocksdb {
    fn from(item: TransactionMined) -> Self {
        Self {
            input: item.input.into(),
            execution: item.execution.into(),
            logs: item.logs.into_iter().map(LogMinedRocksdb::from).collect(),
            transaction_index: IndexRocksdb::from(item.transaction_index),
            block_number: BlockNumberRocksdb::from(item.block_number),
            block_hash: HashRocksdb::from(item.block_hash),
        }
    }
}

impl From<TransactionMinedRocksdb> for TransactionMined {
    fn from(item: TransactionMinedRocksdb) -> Self {
        Self {
            input: item.input.into(),
            execution: item.execution.into(),
            logs: item.logs.into_iter().map(LogMined::from).collect(),
            transaction_index: item.transaction_index.into(),
            block_number: item.block_number.into(),
            block_hash: item.block_hash.into(),
        }
    }
}
