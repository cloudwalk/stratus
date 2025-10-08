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
use crate::eth::primitives::Index;
use crate::eth::primitives::MinedData;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::TransactionMined;
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

impl From<TransactionMined> for TransactionMinedRocksdb {
    fn from(item: TransactionMined) -> Self {
        let execution = item.execution;
        Self {
            input: TransactionInputRocksdb {
                tx_type: execution.info.tx_type.map(|inner| inner.as_u64() as u8),
                chain_id: execution.evm_input.chain_id.map_into(),
                hash: execution.info.hash.into(),
                nonce: execution.evm_input.nonce.unwrap_or_default().into(),
                signer: execution.evm_input.from.into(),
                from: execution.evm_input.from.into(),
                to: execution.evm_input.to.map_into(),
                value: execution.evm_input.value.into(),
                input: execution.evm_input.data.clone().into(),
                gas_limit: execution.evm_input.gas_limit.into(),
                gas_price: execution.evm_input.gas_price.into(),
                v: execution.signature.v.as_u64(),
                r: execution.signature.r.into_limbs(),
                s: execution.signature.s.into_limbs(),
            },
            execution: ExecutionRocksdb::new(
                execution.evm_input.block_timestamp.into(),
                execution.result.execution.result.into(),
                execution.result.execution.output.into(),
                execution.result.execution.gas_used.into(),
                execution.result.execution.deployed_contract_address.map_into(),
            ),
            logs: execution
                .result
                .execution
                .logs
                .into_iter()
                .enumerate()
                .map(|(idx, log)| (log, item.mined_data.first_log_index + Index(idx as u64)).into())
                .collect(),
            transaction_index: item.mined_data.index.into(),
        }
    }
}

impl TransactionMined {
    pub fn from_rocks_primitives(other: TransactionMinedRocksdb, block_number: BlockNumberRocksdb, block_hash: HashRocksdb) -> Self {
        let mined_data = MinedData {
            first_log_index: other.logs.first().map(|log| log.index).unwrap_or_default().into(),
            index: other.transaction_index.into(),
            block_hash: block_hash.into(),
        };

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

        let execution = TransactionExecution {
            info: input.transaction_info,
            signature: input.signature,
            evm_input: EvmInput::from_eth_transaction(&input.execution_info, block_number.into(), other.execution.block_timestamp.into()),
            result: evm_result,
        };

        Self { execution, mined_data }
    }
}

impl SerializeDeserializeWithContext for TransactionMinedRocksdb {}
