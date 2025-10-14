use display_json::DebugAsJson;

use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::EvmExecutionMetrics;

/// Evm execution result.
#[derive(DebugAsJson, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq, Eq)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct EvmExecutionResult {
    pub execution: EvmExecution,
    pub metrics: EvmExecutionMetrics,
}
