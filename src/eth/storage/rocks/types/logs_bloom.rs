use std::fmt::Debug;
use std::fmt::Display;
use std::str::FromStr;

use anyhow::anyhow;
use ethereum_types::Bloom;

use crate::eth::primitives::logs_bloom::LogsBloom;
use crate::gen_newtype_from;

#[derive(Debug, Clone, PartialEq, Eq, serde_with::DeserializeFromStr, serde_with::SerializeDisplay)]
pub struct LogsBloomRocksdb([u8; 256]);

gen_newtype_from!(self = LogsBloomRocksdb, other = Bloom, LogsBloom);

impl From<LogsBloomRocksdb> for LogsBloom {
    fn from(value: LogsBloomRocksdb) -> Self {
        value.0.into()
    }
}

impl Display for LogsBloomRocksdb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", const_hex::encode_prefixed(self.0))
    }
}

impl FromStr for LogsBloomRocksdb {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes: [u8; 256] = const_hex::decode(s)?
            .try_into()
            .map_err(|err| anyhow!("conversion from Vec<u8> to [u8; 256] failed. {:?}", err))?;
        Ok(Self(bytes))
    }
}
