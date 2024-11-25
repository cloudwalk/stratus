use std::fmt::Debug;

use super::block_number::BlockNumberRocksdb;
use super::hash::HashRocksdb;
use super::index::IndexRocksdb;
use super::log::LogRocksdb;
use crate::eth::primitives::LogMined;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct LogMinedRockdb {
    pub log: LogRocksdb,
    pub transaction_hash: HashRocksdb,
    pub transaction_index: IndexRocksdb,
    pub log_index: IndexRocksdb,
    pub block_number: BlockNumberRocksdb,
    pub block_hash: HashRocksdb,
}

impl From<LogMined> for LogMinedRockdb {
    fn from(item: LogMined) -> Self {
        Self {
            log: item.log.into(),
            transaction_hash: item.transaction_hash.into(),
            transaction_index: item.transaction_index.into(),
            log_index: item.log_index.into(),
            block_number: item.block_number.into(),
            block_hash: item.block_hash.into(),
        }
    }
}

impl From<LogMinedRockdb> for LogMined {
    fn from(item: LogMinedRockdb) -> Self {
        Self {
            log: item.log.into(),
            transaction_hash: item.transaction_hash.into(),
            transaction_index: item.transaction_index.into(),
            log_index: item.log_index.into(),
            block_number: item.block_number.into(),
            block_hash: item.block_hash.into(),
        }
    }
}
