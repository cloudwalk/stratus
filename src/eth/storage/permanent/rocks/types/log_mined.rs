use std::fmt::Debug;

use super::block_number::BlockNumberRocksdb;
use super::hash::HashRocksdb;
use super::index::IndexRocksdb;
use super::log::LogRocksdb;
use crate::eth::primitives::LogMined;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct LogMinedRocksdb {
    pub log: LogRocksdb,
    pub transaction_hash: HashRocksdb,
    pub transaction_index: IndexRocksdb,
    pub log_index: IndexRocksdb,
}

impl From<LogMined> for LogMinedRocksdb {
    fn from(item: LogMined) -> Self {
        Self {
            log: item.log.into(),
            transaction_hash: item.transaction_hash.into(),
            transaction_index: item.transaction_index.into(),
            log_index: item.log_index.into(),
        }
    }
}

impl LogMined {
    pub fn from_rocks_primitives(other: LogMinedRocksdb, block_number: BlockNumberRocksdb, block_hash: HashRocksdb) -> Self {
        Self {
            block_number: block_number.into(),
            block_hash: block_hash.into(),
            log: other.log.into(),
            transaction_hash: other.transaction_hash.into(),
            transaction_index: other.transaction_index.into(),
            log_index: other.log_index.into(),
        }
    }
}
