use std::fmt::Debug;

use super::block_number::BlockNumberRocksdb;
use super::execution::ExecutionRocksdb;
use super::hash::HashRocksdb;
use super::index::IndexRocksdb;
use super::log_mined::LogMinedRocksdb;
use super::transaction_input::TransactionInputRocksdb;
use crate::eth::executor::EvmExecutionResult;
use crate::eth::executor::EvmInput;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::EvmExecutionMetrics;
use crate::eth::primitives::ExecutionChanges;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;
use crate::eth::storage::permanent::rocks::types::execution_result::ExecutionResultBuilder;
use crate::ext::OptionExt;
use crate::ext::RuintExt;

#[derive(Debug, Clone, PartialEq, Eq, bincode::Encode, bincode::Decode, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct TransactionMinedRocksdb {
    pub input: TransactionInputRocksdb,
    pub execution: ExecutionRocksdb,
    pub logs: Vec<LogMinedRocksdb>,
    pub transaction_index: IndexRocksdb,
}

impl From<(usize, TransactionExecution)> for TransactionMinedRocksdb {
    fn from((idx, item): (usize, TransactionExecution)) -> Self {
        Self {
            input: TransactionInputRocksdb {
                tx_type: item.info.tx_type.map(|inner| inner.as_u64() as u8),
                chain_id: item.evm_input.chain_id.map_into(),
                hash: item.info.hash.into(),
                nonce: item.evm_input.nonce.unwrap_or_default().into(),
                signer: item.evm_input.from.into(),
                from: item.evm_input.from.into(),
                to: item.evm_input.to.map_into(),
                value: item.evm_input.value.into(),
                input: item.evm_input.data.clone().into(),
                gas_limit: item.evm_input.gas_limit.into(),
                gas_price: item.evm_input.gas_price.into(),
                v: item.signature.v.as_u64(),
                r: item.signature.r.into_limbs(),
                s: item.signature.s.into_limbs(),
            },
            execution: ExecutionRocksdb::new(
                item.evm_input.block_timestamp.into(),
                item.result.execution.result.into(),
                item.result.execution.output.into(),
                item.result.execution.gas_used.into(),
                item.result.execution.deployed_contract_address.map_into(),
            ),
            logs: item.result.execution.logs.into_iter().map(|log| log.into()).collect(),
            transaction_index: IndexRocksdb(idx as u32),
        }
    }
}

impl TransactionExecution {
    pub fn from_rocks_primitives(other: TransactionMinedRocksdb, block_number: BlockNumberRocksdb, block_hash: HashRocksdb) -> Self {
        let logs = other.logs.into_iter().map(|log| log.into()).collect();

        let (result, output) = ExecutionResultBuilder((other.execution.result, other.execution.output)).build();

        let input = TransactionInput::from(other.input);
        let evm_result = EvmExecutionResult {
            execution: EvmExecution {
                block_timestamp: other.execution.block_timestamp.into(),
                result,
                output,
                logs,
                gas_used: other.execution.gas.into(),
                changes: ExecutionChanges::default(),
                deployed_contract_address: other.execution.deployed_contract_address.map_into(),
            },
            metrics: EvmExecutionMetrics::default(),
        };

        Self {
            info: input.transaction_info,
            signature: input.signature,
            evm_input: EvmInput::from_eth_transaction(&input.execution_info, block_number.into(), other.execution.block_timestamp.into()),
            result: evm_result,
            index: other.transaction_index.into(),
            block_hash: Some(block_hash.into())
        }
    }
}

impl SerializeDeserializeWithContext for TransactionMinedRocksdb {}
