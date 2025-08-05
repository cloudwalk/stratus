use itertools::Itertools;
use serde::Deserialize;

use crate::eth::storage::permanent::rocks::cf_versions::CfBlocksByNumberValue;
use crate::eth::storage::permanent::rocks::types::AddressRocksdb;
use crate::eth::storage::permanent::rocks::types::BytesRocksdb;
use crate::eth::storage::permanent::rocks::types::IndexRocksdb;
use crate::eth::storage::permanent::rocks::types::TransactionMinedRocksdb;
use crate::eth::storage::permanent::rocks::types::UnixTimeRocksdb;
use crate::eth::storage::permanent::rocks::types::block::BlockRocksdb;
use crate::eth::storage::permanent::rocks::types::block_header::BlockHeaderRocksdb;
use crate::eth::storage::permanent::rocks::types::execution::ExecutionRocksdb;
use crate::eth::storage::permanent::rocks::types::execution_result::ExecutionResultRocksdb;
use crate::eth::storage::permanent::rocks::types::gas::GasRocksdb;
use crate::eth::storage::permanent::rocks::types::log::LogRocksdb;
use crate::eth::storage::permanent::rocks::types::log_mined::LogMinedRocksdb;
use crate::eth::storage::permanent::rocks::types::transaction_input::TransactionInputRocksdb;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy, bincode::Encode, bincode::Decode)]
pub struct OldExecutionRocksdb {
    block_timestamp: UnixTimeRocksdb,
    execution_costs_applied: bool,
    result: ExecutionResultRocksdb,
    output: BytesRocksdb,
    logs: Vec<LogRocksdb>,
    gas: GasRocksdb,
    deployed_contract_address: Option<AddressRocksdb>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy, bincode::Encode, bincode::Decode)]
pub struct OldTransactionMinedRocksdb {
    input: TransactionInputRocksdb,
    execution: OldExecutionRocksdb,
    logs: Vec<LogMinedRocksdb>,
    transaction_index: IndexRocksdb,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, fake::Dummy, bincode::Encode, bincode::Decode)]
pub struct OldBlockRocksdb {
    header: BlockHeaderRocksdb,
    transactions: Vec<OldTransactionMinedRocksdb>,
}

impl From<OldExecutionRocksdb> for ExecutionRocksdb {
    fn from(old: OldExecutionRocksdb) -> Self {
        ExecutionRocksdb {
            block_timestamp: old.block_timestamp,
            result: old.result,
            output: old.output,
            logs: old.logs,
            gas: old.gas,
            deployed_contract_address: old.deployed_contract_address,
        }
    }
}

impl From<OldTransactionMinedRocksdb> for TransactionMinedRocksdb {
    fn from(old: OldTransactionMinedRocksdb) -> Self {
        TransactionMinedRocksdb {
            input: old.input,
            execution: old.execution.into(),
            logs: old.logs,
            transaction_index: old.transaction_index,
        }
    }
}

impl From<OldBlockRocksdb> for BlockRocksdb {
    fn from(old: OldBlockRocksdb) -> Self {
        BlockRocksdb {
            header: old.header,
            transactions: old.transactions.into_iter().map_into().collect(),
        }
    }
}

impl From<OldCfBlocksByNumberValue> for CfBlocksByNumberValue {
    fn from(old: OldCfBlocksByNumberValue) -> Self {
        match old {
            OldCfBlocksByNumberValue::V1(inner) => CfBlocksByNumberValue::V1(inner.into()),
        }
    }
}

#[derive(Deserialize, bincode::Encode, bincode::Decode)]
pub enum OldCfBlocksByNumberValue {
    V1(OldBlockRocksdb),
}
