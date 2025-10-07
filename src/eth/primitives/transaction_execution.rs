use alloy_consensus::Eip658Value;
use alloy_consensus::Receipt;
use alloy_consensus::ReceiptEnvelope;
use alloy_consensus::ReceiptWithBloom;
use display_json::DebugAsJson;

use crate::alias::AlloyLog;
use crate::alias::AlloyLogData;
use crate::alias::AlloyLogPrimitive;
use crate::alias::AlloyReceipt;
use crate::alias::AlloyTransaction;
use crate::alias::JsonValue;
use crate::eth::executor::EvmExecutionResult;
use crate::eth::executor::EvmInput;
use crate::eth::primitives::EvmExecutionMetrics;
use crate::eth::primitives::ExecutionInfo;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::Log;
use crate::eth::primitives::Signature;
use crate::eth::primitives::TransactionInfo;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::logs_bloom::LogsBloom;
use crate::ext::OptionExt;
use crate::ext::to_json_value;

#[derive(DebugAsJson, Clone, derive_new::new, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct TransactionExecution {
    pub info: TransactionInfo,
    pub signature: Signature,
    pub evm_input: EvmInput,
    pub result: EvmExecutionResult,
    // mined data
    pub index: Index,
    pub block_hash: Option<Hash>,
}

impl TransactionExecution {
    /// Returns the EVM execution metrics.
    pub fn metrics(&self) -> EvmExecutionMetrics {
        self.result.metrics
    }

    pub fn create_alloy_logs(&self) -> Vec<AlloyLog> {
        self.logs()
            .iter()
            .map(|log| AlloyLog {
                inner: AlloyLogPrimitive {
                    address: log.address.into(),
                    data: AlloyLogData::new_unchecked(log.topics_non_empty().into_iter().map(Into::into).collect(), log.data.clone().into()),
                },
                block_hash: self.block_hash.map_into(),
                block_number: Some(self.evm_input.block_number.as_u64()),
                block_timestamp: Some(*self.evm_input.block_timestamp),
                transaction_hash: Some(self.info.hash.into()),
                transaction_index: Some(self.index.into()),
                log_index: log.index.map_into(),
                removed: false,
            })
            .collect()
    }

    /// Computes the bloom filter from execution logs.
    fn compute_bloom(&self) -> LogsBloom {
        let mut bloom = LogsBloom::default();
        for log in self.result.execution.logs.iter() {
            bloom.accrue_log(log);
        }
        bloom
    }

    pub fn to_json_rpc_receipt(self) -> JsonValue {
        to_json_value(AlloyReceipt::from(self))
    }

    pub fn to_json_rpc_transaction(self) -> JsonValue {
        to_json_value(AlloyTransaction::from(self))
    }

    pub fn logs(&self) -> &Vec<Log> {
        &self.result.execution.logs
    }
}

impl From<TransactionExecution> for AlloyTransaction {
    fn from(value: TransactionExecution) -> Self {
        let tx_input: TransactionInput = value.into();
        tx_input.into()
    }
}

impl From<TransactionExecution> for TransactionInput {
    fn from(value: TransactionExecution) -> Self {
        Self {
            transaction_info: value.info,
            execution_info: ExecutionInfo {
                chain_id: value.evm_input.chain_id,
                nonce: value.evm_input.nonce.unwrap_or_default(),
                signer: value.evm_input.from,
                to: value.evm_input.to,
                value: value.evm_input.value,
                input: value.evm_input.data,
                gas_limit: value.evm_input.gas_limit,
                gas_price: value.evm_input.gas_price,
            },
            signature: value.signature,
        }
    }
}

impl From<TransactionExecution> for AlloyReceipt {
    fn from(value: TransactionExecution) -> Self {
        let alloy_logs = value.create_alloy_logs();

        let receipt = Receipt {
            status: Eip658Value::Eip658(value.result.execution.is_success()),
            cumulative_gas_used: value.result.execution.gas_used.into(),
            logs: alloy_logs,
        };

        let receipt_with_bloom = ReceiptWithBloom {
            receipt,
            logs_bloom: value.compute_bloom().into(),
        };

        let inner = match value.info.tx_type.and_then(|tx| tx.try_into().ok()) {
            Some(1u64) => ReceiptEnvelope::Eip2930(receipt_with_bloom),
            Some(2u64) => ReceiptEnvelope::Eip1559(receipt_with_bloom),
            Some(3u64) => ReceiptEnvelope::Eip4844(receipt_with_bloom),
            Some(4u64) => ReceiptEnvelope::Eip7702(receipt_with_bloom),
            _ => ReceiptEnvelope::Legacy(receipt_with_bloom),
        };

        Self {
            inner,
            transaction_hash: value.info.hash.into(),
            transaction_index: Some(value.index.into()),
            block_hash: value.block_hash.map_into(),
            block_number: Some(value.evm_input.block_number.as_u64()),
            gas_used: value.result.execution.gas_used.into(),
            effective_gas_price: value.evm_input.gas_price,
            blob_gas_used: None,
            blob_gas_price: None,
            from: value.evm_input.from.into(),
            to: value.evm_input.to.map_into(),
            contract_address: value.result.execution.deployed_contract_address.map_into(),
        }
    }
}
