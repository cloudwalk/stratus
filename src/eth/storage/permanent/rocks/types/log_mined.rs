use std::fmt::Debug;

use super::block_number::BlockNumberRocksdb;
use super::hash::HashRocksdb;
use super::log::LogRocksdb;
use crate::eth::primitives::Index;
use crate::eth::primitives::LogMined;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy)]
pub struct LogMinedRocksdb {
    pub log: LogRocksdb,
    pub index: u64,
}

impl From<LogMined> for LogMinedRocksdb {
    fn from(item: LogMined) -> Self {
        Self {
            log: item.log.into(),
            index: item.log_index.into(),
        }
    }
}

impl LogMined {
    pub fn from_rocks_primitives(
        other: LogRocksdb,
        block_number: BlockNumberRocksdb,
        block_hash: HashRocksdb,
        tx_index: usize,
        tx_hash: HashRocksdb,
        log_index: u64,
    ) -> Self {
        Self {
            block_number: block_number.into(),
            block_hash: block_hash.into(),
            log: other.into(),
            transaction_hash: tx_hash.into(),
            transaction_index: Index::from(tx_index as u64),
            log_index: Index::from(log_index),
        }
    }
}

impl SerializeDeserializeWithContext for LogMinedRocksdb {}
