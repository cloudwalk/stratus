use std::fmt::Debug;

use super::address::AddressRocksdb;
use super::bytes::BytesRocksdb;
use crate::eth::primitives::Log;
use crate::ext::OptionExt;

type LogTopic = [u8; 32];

#[derive(Debug, Clone, PartialEq, Eq, bincode::Encode, bincode::Decode, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct LogRocksdb {
    pub address: AddressRocksdb,
    pub topics: (Option<LogTopic>, Option<LogTopic>, Option<LogTopic>, Option<LogTopic>),
    pub data: BytesRocksdb,
}

impl From<Log> for LogRocksdb {
    fn from(item: Log) -> Self {
        Self {
            address: AddressRocksdb::from(item.address),
            topics: (item.topic0.map_into(), item.topic1.map_into(), item.topic2.map_into(), item.topic3.map_into()),
            data: BytesRocksdb::from(item.data),
        }
    }
}

impl From<LogRocksdb> for Log {
    fn from(item: LogRocksdb) -> Self {
        Self {
            address: item.address.into(),
            topic0: item.topics.0.map_into(),
            topic1: item.topics.1.map_into(),
            topic2: item.topics.2.map_into(),
            topic3: item.topics.3.map_into(),
            data: item.data.into(),
        }
    }
}
