use display_json::DebugAsJson;

use crate::eth::evm::EvmExecutionResult;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::Hash;
use crate::eth::primitives::TransactionInput;

#[allow(clippy::large_enum_variant)]
#[derive(DebugAsJson, Clone, serde::Serialize)]
pub enum TransactionExecution {
    /// Transaction that was sent directly to Stratus.
    Local(LocalTransactionExecution),

    /// Transaction that imported from external source.
    External(ExternalTransactionExecution),
}

impl TransactionExecution {
    /// Creates a new transaction execution from a local transaction.
    pub fn new_local(tx: TransactionInput, result: EvmExecutionResult) -> Self {
        Self::Local(LocalTransactionExecution { input: tx, result })
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
            Self::External(ExternalTransactionExecution { result, .. }) => result,
        }
    }
}

#[derive(DebugAsJson, Clone, derive_new::new, serde::Serialize)]
pub struct LocalTransactionExecution {
    pub input: TransactionInput,
    pub result: EvmExecutionResult,
}

#[derive(DebugAsJson, Clone, derive_new::new, serde::Serialize)]
pub struct ExternalTransactionExecution {
    pub tx: ExternalTransaction,
    pub receipt: ExternalReceipt,
    pub result: EvmExecutionResult,
}
