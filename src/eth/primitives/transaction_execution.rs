use display_json::DebugAsJson;

use crate::eth::executor::EvmExecutionResult;
use crate::eth::executor::EvmInput;
use crate::eth::primitives::EvmExecutionMetrics;
use crate::eth::primitives::TransactionInput;

#[derive(DebugAsJson, Clone, derive_new::new, serde::Serialize)]
#[cfg_attr(test, derive(serde::Deserialize, fake::Dummy, PartialEq))]
pub struct TransactionExecution {
    pub input: TransactionInput,
    pub evm_input: EvmInput,
    pub result: EvmExecutionResult,
}

impl TransactionExecution {
    /// Returns the EVM execution metrics.
    pub fn metrics(&self) -> EvmExecutionMetrics {
        self.result.metrics
    }
}
