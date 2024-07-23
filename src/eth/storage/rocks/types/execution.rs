use std::collections::HashMap;

use super::address::AddressRocksdb;
use super::bytes::BytesRocksdb;
use super::execution_result::ExecutionResultRocksdb;
use super::gas::GasRocksdb;
use super::log::LogRocksdb;
use super::unix_time::UnixTimeRocksdb;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::Log;
use crate::ext::OptionExt;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExecutionRocksdb {
    pub block_timestamp: UnixTimeRocksdb,
    pub execution_costs_applied: bool,
    pub result: ExecutionResultRocksdb,
    pub output: BytesRocksdb,
    pub logs: Vec<LogRocksdb>,
    pub gas: GasRocksdb,
    pub deployed_contract_address: Option<AddressRocksdb>,
}

impl From<EvmExecution> for ExecutionRocksdb {
    fn from(item: EvmExecution) -> Self {
        Self {
            block_timestamp: UnixTimeRocksdb::from(item.block_timestamp),
            execution_costs_applied: item.receipt_applied,
            result: item.result.into(),
            output: BytesRocksdb::from(item.output),
            logs: item.logs.into_iter().map(LogRocksdb::from).collect(),
            gas: GasRocksdb::from(item.gas),
            deployed_contract_address: item.deployed_contract_address.map_into(),
        }
    }
}

impl From<ExecutionRocksdb> for EvmExecution {
    fn from(item: ExecutionRocksdb) -> Self {
        Self {
            block_timestamp: item.block_timestamp.into(),
            receipt_applied: item.execution_costs_applied,
            result: item.result.into(),
            output: item.output.into(),
            logs: item.logs.into_iter().map(Log::from).collect(),
            gas: item.gas.into(),
            changes: Default::default(),
            deployed_contract_address: item.deployed_contract_address.map_into(),
        }
    }
}
