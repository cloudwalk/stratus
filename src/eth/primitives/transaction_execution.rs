use display_json::DebugAsJson;

use crate::eth::executor::EvmExecutionResult;
use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::EvmExecutionMetrics;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::Hash;
use crate::eth::primitives::TransactionInput;

#[allow(clippy::large_enum_variant)]
#[derive(DebugAsJson, Clone, strum::EnumIs, serde::Serialize)]
pub enum TransactionExecution {
    /// Transaction that was sent directly to Stratus.
    Local(LocalTransactionExecution),

    /// Transaction that imported from external source.
    External(ExternalTransactionExecution),
}

impl TransactionExecution {
    /// Creates a new local transaction execution.
    pub fn new_local(tx: TransactionInput, result: EvmExecutionResult) -> Self {
        Self::Local(LocalTransactionExecution { input: tx, result })
    }

    /// Extracts the inner [`LocalTransactionExecution`] if the execution is a local execution.
    pub fn as_local(self) -> Option<LocalTransactionExecution> {
        match self {
            Self::Local(inner) => Some(inner),
            _ => None,
        }
    }

    /// Checks if the current transaction was completed normally.
    pub fn is_success(&self) -> bool {
        match self {
            Self::Local(inner) => inner.is_success(),
            Self::External(inner) => inner.is_success(),
        }
    }

    /// Checks if the current transaction was completed with a failure (reverted or halted).
    pub fn is_failure(&self) -> bool {
        match self {
            Self::Local(inner) => inner.is_failure(),
            Self::External(inner) => inner.is_failure(),
        }
    }

    /// Returns the transaction hash.
    pub fn hash(&self) -> Hash {
        match self {
            Self::Local(LocalTransactionExecution { input, .. }) => input.hash,
            Self::External(ExternalTransactionExecution { tx, .. }) => tx.hash(),
        }
    }

    /// Returns the execution result.
    pub fn result(&self) -> &EvmExecutionResult {
        match self {
            Self::Local(LocalTransactionExecution { result, .. }) => result,
            Self::External(ExternalTransactionExecution { evm_execution: result, .. }) => result,
        }
    }

    /// Returns the EVM execution.
    pub fn execution(&self) -> &EvmExecution {
        match self {
            Self::Local(LocalTransactionExecution { result, .. }) => &result.execution,
            Self::External(ExternalTransactionExecution { evm_execution: result, .. }) => &result.execution,
        }
    }

    /// Returns the EVM execution metrics.
    pub fn metrics(&self) -> &EvmExecutionMetrics {
        match self {
            Self::Local(LocalTransactionExecution { result, .. }) => &result.metrics,
            Self::External(ExternalTransactionExecution { evm_execution: result, .. }) => &result.metrics,
        }
    }
}

#[derive(DebugAsJson, Clone, derive_new::new, serde::Serialize)]
pub struct LocalTransactionExecution {
    pub input: TransactionInput,
    pub result: EvmExecutionResult,
}

impl LocalTransactionExecution {
    /// Check if the current transaction was completed normally.
    pub fn is_success(&self) -> bool {
        self.result.is_success()
    }

    /// Checks if the current transaction was completed with a failure (reverted or halted).
    pub fn is_failure(&self) -> bool {
        self.result.is_failure()
    }
}

#[derive(DebugAsJson, Clone, derive_new::new, serde::Serialize)]
pub struct ExternalTransactionExecution {
    pub tx: ExternalTransaction,
    pub receipt: ExternalReceipt,
    pub evm_execution: EvmExecutionResult,
}

impl ExternalTransactionExecution {
    /// Check if the current transaction was completed normally.
    pub fn is_success(&self) -> bool {
        self.evm_execution.is_success()
    }

    /// Checks if the current transaction was completed with a failure (reverted or halted).
    pub fn is_failure(&self) -> bool {
        self.evm_execution.is_failure()
    }
}
