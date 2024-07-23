use std::fmt::Debug;

use ethereum_types::Bloom;

use crate::eth::primitives::logs_bloom::LogsBloom;
use crate::gen_newtype_from;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(transparent)]
pub struct LogsBloomRocksdb(Bloom);

gen_newtype_from!(self = LogsBloomRocksdb, other = Bloom, LogsBloom);

impl From<LogsBloomRocksdb> for LogsBloom {
    fn from(value: LogsBloomRocksdb) -> Self {
        value.0.into()
    }
}
