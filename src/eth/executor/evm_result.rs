use display_json::DebugAsJson;

use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::EvmExecutionMetrics;

/// Evm execution result.
#[derive(DebugAsJson, Clone, serde::Serialize)]
#[cfg_attr(test, derive(serde::Deserialize, fake::Dummy, PartialEq))]
pub struct EvmExecutionResult {
    pub execution: EvmExecution,
    pub metrics: EvmExecutionMetrics,
}
