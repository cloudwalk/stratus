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
use crate::eth::executor::EvmExecutionResult;
use crate::eth::executor::EvmInput;
use crate::eth::primitives::EvmExecutionMetrics;
use crate::eth::primitives::ExecutionInfo;
use crate::eth::primitives::Log;
use crate::eth::primitives::MinedData;
use crate::eth::primitives::Signature;
use crate::eth::primitives::TransactionInfo;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::logs_bloom::LogsBloom;
use crate::ext::OptionExt;

#[derive(DebugAsJson, Clone, derive_new::new, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct TransactionExecution {
    pub info: TransactionInfo,
    pub signature: Signature,
    pub evm_input: EvmInput,
    pub result: EvmExecutionResult,
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
                block_hash: None,
                block_number: Some(self.evm_input.block_number.as_u64()),
                block_timestamp: Some(*self.evm_input.block_timestamp),
                transaction_hash: Some(self.info.hash.into()),
                transaction_index: None,
                log_index: None,
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

pub fn _tx_to_alloy_receipt_impl(execution: TransactionExecution, alloy_logs: Vec<AlloyLog>, mined_data: Option<MinedData>) -> AlloyReceipt {
    let receipt = Receipt {
        status: Eip658Value::Eip658(execution.result.execution.is_success()),
        cumulative_gas_used: execution.result.execution.gas_used.into(),
        logs: alloy_logs,
    };

    let receipt_with_bloom = ReceiptWithBloom {
        receipt,
        logs_bloom: execution.compute_bloom().into(),
    };

    let inner = match execution.info.tx_type.and_then(|tx| tx.try_into().ok()) {
        Some(1u64) => ReceiptEnvelope::Eip2930(receipt_with_bloom),
        Some(2u64) => ReceiptEnvelope::Eip1559(receipt_with_bloom),
        Some(3u64) => ReceiptEnvelope::Eip4844(receipt_with_bloom),
        Some(4u64) => ReceiptEnvelope::Eip7702(receipt_with_bloom),
        _ => ReceiptEnvelope::Legacy(receipt_with_bloom),
    };

    AlloyReceipt {
        inner,
        transaction_hash: execution.info.hash.into(),
        transaction_index: mined_data.map(|data| data.index.into()),
        block_hash: mined_data.map(|data| data.block_hash.into()),
        block_number: Some(execution.evm_input.block_number.as_u64()),
        gas_used: execution.result.execution.gas_used.into(),
        effective_gas_price: execution.evm_input.gas_price,
        blob_gas_used: None,
        blob_gas_price: None,
        from: execution.evm_input.from.into(),
        to: execution.evm_input.to.map_into(),
        contract_address: execution.result.execution.deployed_contract_address.map_into(),
    }
}

impl From<TransactionExecution> for AlloyReceipt {
    fn from(value: TransactionExecution) -> Self {
        let alloy_logs = value.create_alloy_logs();
        _tx_to_alloy_receipt_impl(value, alloy_logs, None)
    }
}
