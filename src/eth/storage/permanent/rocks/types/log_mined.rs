use std::fmt::Debug;

use super::block_number::BlockNumberRocksdb;
use super::hash::HashRocksdb;
use super::log::LogRocksdb;
use crate::eth::primitives::{Index, Log, LogMessage};
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;

#[derive(Debug, Clone, PartialEq, Eq, bincode::Encode, bincode::Decode, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct LogMinedRocksdb {
    pub log: LogRocksdb,
    pub index: u64,
}

impl From<(Log, u64)> for LogMinedRocksdb {
    fn from((log, index): (Log, u64)) -> Self {
        Self {
            log: log.into(),
            index,
        }
    }
}

impl LogMessage {
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
