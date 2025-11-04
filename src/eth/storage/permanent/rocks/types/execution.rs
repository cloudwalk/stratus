#![allow(deprecated)]

use super::address::AddressRocksdb;
use super::bytes::BytesRocksdb;
use super::execution_result::ExecutionResultRocksdb;
use super::gas::GasRocksdb;
use super::log::LogRocksdb;
use super::unix_time::UnixTimeRocksdb;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;

#[derive(Debug, Clone, PartialEq, Eq, bincode::Encode, bincode::Decode, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct ExecutionRocksdb {
    pub block_timestamp: UnixTimeRocksdb,
    pub result: ExecutionResultRocksdb,
    pub output: BytesRocksdb,
    #[deprecated(note = "Use logs in transaction mined instead")]
    logs: Vec<LogRocksdb>,
    pub gas: GasRocksdb,
    pub deployed_contract_address: Option<AddressRocksdb>,
}

impl ExecutionRocksdb {
    pub fn new(
        block_timestamp: UnixTimeRocksdb,
        result: ExecutionResultRocksdb,
        output: BytesRocksdb,
        gas: GasRocksdb,
        deployed_contract_address: Option<AddressRocksdb>,
    ) -> Self {
        Self {
            block_timestamp,
            result,
            output,
            logs: Vec::new(),
            gas,
            deployed_contract_address,
        }
    }
}

impl SerializeDeserializeWithContext for ExecutionRocksdb {}
