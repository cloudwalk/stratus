use std::fmt::Debug;

use super::block_number::BlockNumberRocksdb;
use super::execution::ExecutionRocksdb;
use super::hash::HashRocksdb;
use super::index::IndexRocksdb;
use super::log_mined::LogMinedRocksdb;
use super::transaction_input::TransactionInputRocksdb;
use crate::eth::executor::TransactionExecution;
use crate::eth::executor::TransactionExecutionInput;
use crate::eth::executor::TransactionExecutionResult;
use crate::eth::storage::permanent::rocks::SerializeDeserializeWithContext;
use crate::eth::storage::permanent::rocks::types::execution_result::ExecutionResultBuilder;
use crate::eth::types::BlockInfo;
use crate::eth::types::Index;
use crate::eth::types::MinedData;
use crate::eth::types::TransactionInput;
use crate::eth::types::TransactionMined;
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
                chain_id: execution.input.chain_id.map_into(),
                hash: execution.info.hash.into(),
                nonce: execution.input.nonce.into(),
                signer: execution.input.from.into(),
                from: execution.input.from.into(),
                to: execution.input.to.map_into(),
                value: execution.input.value.into(),
                input: execution.input.data.clone().into(),
                gas_limit: execution.input.gas_limit.into(),
                gas_price: execution.input.gas_price.into(),
                v: execution.signature.v.as_u64(),
                r: execution.signature.r.into_limbs(),
                s: execution.signature.s.into_limbs(),
            },
            execution: ExecutionRocksdb::new(
                execution.input.block_timestamp.into(),
                execution.output.result.into(),
                execution.output.output.into(),
                execution.output.gas_used.into(),
                execution.output.deployed_contract_address.map_into(),
            ),
            logs: execution
                .output
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
        let evm_result = TransactionExecutionResult {
            result,
            output,
            logs,
            gas_used: other.execution.gas.into(),
            deployed_contract_address: other.execution.deployed_contract_address.map_into(),
        };

        let evm_input = TransactionExecutionInput::create(
            &input,
            BlockInfo {
                number: block_number.into(),
                timestamp: other.execution.block_timestamp.into(),
            },
        );
        let execution = TransactionExecution {
            info: input.transaction_info,
            signature: input.signature,
            input: evm_input,
            output: evm_result,
        };

        Self { execution, mined_data }
    }
}

impl SerializeDeserializeWithContext for TransactionMinedRocksdb {}
