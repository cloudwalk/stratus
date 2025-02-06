use display_json::DebugAsJson;

use crate::eth::executor::EvmExecutionResult;
use crate::eth::executor::EvmInput;
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
    pub fn new_local(tx: TransactionInput, evm_input: EvmInput, result: EvmExecutionResult) -> Self {
        Self::Local(LocalTransactionExecution { input: tx, evm_input, result })
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
    pub fn metrics(&self) -> EvmExecutionMetrics {
        match self {
            Self::Local(LocalTransactionExecution { result, .. }) => result.metrics,
            Self::External(ExternalTransactionExecution { evm_execution: result, .. }) => result.metrics,
        }
    }
}

#[derive(DebugAsJson, Clone, derive_new::new, serde::Serialize)]
#[cfg_attr(test, derive(serde::Deserialize, fake::Dummy, PartialEq))]
pub struct LocalTransactionExecution {
    pub input: TransactionInput,
    pub evm_input: EvmInput,
    pub result: EvmExecutionResult,
}

#[derive(DebugAsJson, Clone, derive_new::new, serde::Serialize)]
pub struct ExternalTransactionExecution {
    pub tx: ExternalTransaction,
    pub receipt: ExternalReceipt,
    pub evm_execution: EvmExecutionResult,
}
