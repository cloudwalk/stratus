use display_json::DebugAsJson;

use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::EvmExecutionMetrics;

/// Evm execution result.
#[derive(DebugAsJson, Clone, serde::Serialize)]
pub struct EvmExecutionResult {
    pub execution: EvmExecution,
    pub metrics: EvmExecutionMetrics,
}

impl EvmExecutionResult {
    /// Checks if the current transaction was completed normally.
    pub fn is_success(&self) -> bool {
        self.execution.is_success()
    }

    /// Checks if the current transaction was completed with a failure (reverted or halted).
    pub fn is_failure(&self) -> bool {
        self.execution.is_failure()
    }
}
