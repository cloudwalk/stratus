use derive_more::Deref;
use display_json::DebugAsJson;

use crate::alias::AlloyLog;
use crate::alias::AlloyLogData;
use crate::alias::AlloyLogPrimitive;
use crate::alias::AlloyReceipt;
use crate::alias::AlloyTransaction;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::transaction_execution::_tx_to_alloy_receipt_impl;

#[derive(DebugAsJson, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct MinedData {
    pub index: Index,
    pub first_log_index: Index,
    pub block_hash: Hash,
}

#[derive(DebugAsJson, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Deref)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct TransactionMined {
    #[deref]
    pub execution: TransactionExecution,
    pub mined_data: MinedData,
}

impl TransactionMined {
    pub fn create_alloy_logs(&self) -> Vec<AlloyLog> {
        self.logs()
            .iter()
            .enumerate()
            .map(|(idx, log)| AlloyLog {
                inner: AlloyLogPrimitive {
                    address: log.address.into(),
                    data: AlloyLogData::new_unchecked(log.topics_non_empty().into_iter().map(Into::into).collect(), log.data.clone().into()),
                },
                block_hash: Some(self.mined_data.block_hash.into()),
                block_number: Some(self.evm_input.block_number.as_u64()),
                block_timestamp: Some(*self.evm_input.block_timestamp),
                transaction_hash: Some(self.info.hash.into()),
                transaction_index: Some(*self.mined_data.index),
                log_index: Some(*self.mined_data.first_log_index + idx as u64),
                removed: false,
            })
            .collect()
    }

    pub fn from_execution(execution: TransactionExecution, block_hash: Hash, tx_index: Index, first_log_index: Index) -> Self {
        Self {
            execution,
            mined_data: MinedData {
                index: tx_index,
                first_log_index,
                block_hash,
            },
        }
    }
}

impl From<TransactionMined> for AlloyTransaction {
    fn from(value: TransactionMined) -> Self {
        let tx_input: TransactionInput = value.into();
        tx_input.into()
    }
}

impl From<TransactionMined> for TransactionInput {
    fn from(value: TransactionMined) -> Self {
        value.execution.into()
    }
}

impl From<TransactionMined> for AlloyReceipt {
    fn from(value: TransactionMined) -> Self {
        let alloy_logs = value.create_alloy_logs();
        _tx_to_alloy_receipt_impl(value.execution, alloy_logs, Some(value.mined_data))
    }
}
