use display_json::DebugAsJson;

use crate::alias::AlloyTransaction;
use crate::eth::executor::EvmExecutionResult;
use crate::eth::executor::EvmInput;
use crate::eth::primitives::EvmExecutionMetrics;
use crate::eth::primitives::ExecutionInfo;
use crate::eth::primitives::Signature;
use crate::eth::primitives::TransactionInfo;
use crate::eth::primitives::TransactionInput;

#[derive(DebugAsJson, Clone, derive_new::new, serde::Serialize)]
#[cfg_attr(test, derive(serde::Deserialize, fake::Dummy, PartialEq))]
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
