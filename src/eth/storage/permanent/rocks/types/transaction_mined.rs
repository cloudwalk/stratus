use std::fmt::Debug;

use super::block_number::BlockNumberRocksdb;
use super::execution::ExecutionRocksdb;
use super::hash::HashRocksdb;
use super::index::IndexRocksdb;
use super::log_mined::LogMinedRocksdb;
use super::transaction_input::TransactionInputRocksdb;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::TransactionMined;
use crate::eth::storage::permanent::rocks::types::execution_result::ExecutionResultBuilder;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;
use crate::ext::OptionExt;

#[derive(Debug, Clone, PartialEq, Eq, bincode::Encode, bincode::Decode, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct TransactionMinedRocksdb {
    pub input: TransactionInputRocksdb,
    pub execution: ExecutionRocksdb,
    pub logs: Vec<LogMinedRocksdb>,
    pub transaction_index: IndexRocksdb,
}

impl From<TransactionMined> for TransactionMinedRocksdb {
    fn from(item: TransactionMined) -> Self {
        Self {
            input: item.input.into(),
            execution: ExecutionRocksdb {
                block_timestamp: item.block_timestamp.into(),
                result: item.result.into(),
                output: item.output.into(),
                logs: item.logs.iter().cloned().map(|log| log.log.into()).collect(),
                gas: item.gas.into(),
                deployed_contract_address: item.deployed_contract_address.map_into(),
            },
            logs: item.logs.into_iter().map(LogMinedRocksdb::from).collect(), // TODO: remove duplicated logs from execution
            transaction_index: IndexRocksdb::from(item.transaction_index),
        }
    }
}

impl TransactionMined {
    pub fn from_rocks_primitives(other: TransactionMinedRocksdb, block_number: BlockNumberRocksdb, block_hash: HashRocksdb) -> Self {
        let logs = other
            .logs
            .into_iter()
            .map(|log| {
                LogMined::from_rocks_primitives(
                    log.log,
                    block_number,
                    block_hash,
                    other.transaction_index.as_usize(),
                    other.input.hash,
                    log.index,
                )
            })
            .collect();

        let (result, output) = ExecutionResultBuilder((other.execution.result, other.execution.output)).build();
        Self {
            block_number: block_number.into(),
            block_hash: block_hash.into(),
            input: other.input.into(),
            block_timestamp: other.execution.block_timestamp.into(),
            result,
            output,
            gas: other.execution.gas.into(),
            deployed_contract_address: other.execution.deployed_contract_address.map_into(),
            transaction_index: other.transaction_index.into(),
            logs,
        }
    }
}

impl SerializeDeserializeWithContext for TransactionMinedRocksdb {}
