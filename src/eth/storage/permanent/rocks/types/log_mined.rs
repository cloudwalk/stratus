use std::fmt::Debug;

use super::block_number::BlockNumberRocksdb;
use super::hash::HashRocksdb;
use super::log::LogRocksdb;
use crate::eth::primitives::Index;
use crate::eth::primitives::Log;
use crate::eth::primitives::LogMessage;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;
use crate::ext::OptionExt;

#[derive(Debug, Clone, PartialEq, Eq, bincode::Encode, bincode::Decode, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct LogMinedRocksdb {
    pub log: LogRocksdb,
    pub index: u64,
}

impl From<Log> for LogMinedRocksdb {
    fn from(log: Log) -> Self {
        let index = log.index.unwrap_or_default().into();
        Self { log: log.into(), index }
    }
}

impl From<LogMinedRocksdb> for Log {
    fn from(value: LogMinedRocksdb) -> Self {
        Self {
            address: value.log.address.into(),
            topic0: value.log.topics.0.map_into(),
            topic1: value.log.topics.1.map_into(),
            topic2: value.log.topics.2.map_into(),
            topic3: value.log.topics.3.map_into(),
            data: value.log.data.into(),
            index: Some(value.index.into()),
        }
    }
}

impl LogMessage {
    pub fn from_rocks_primitives(
        log: LogMinedRocksdb,
        block_number: BlockNumberRocksdb,
        block_hash: HashRocksdb,
        tx_index: usize,
        tx_hash: HashRocksdb,
    ) -> Self {
        Self {
            block_number: block_number.into(),
            block_hash: block_hash.into(),
            log: log.into(),
            transaction_hash: tx_hash.into(),
            transaction_index: Index::from(tx_index as u64),
        }
    }
}

impl SerializeDeserializeWithContext for LogMinedRocksdb {}
