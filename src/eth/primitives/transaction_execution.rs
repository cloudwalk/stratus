use display_json::DebugAsJson;

use crate::eth::evm::EvmExecutionResult;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::Hash;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::TransactionKind;

#[derive(DebugAsJson, Clone, serde::Serialize)]
pub struct TransactionExecution {
    pub kind: TransactionKind,
    pub result: EvmExecutionResult,
}

impl TransactionExecution {
    /// Creates a new transaction execution from a local transaction.
    pub fn from_local(tx: TransactionInput, result: EvmExecutionResult) -> Self {
        Self {
            kind: TransactionKind::new_local(tx),
            result,
        }
    }

    /// Creates a new transaction execution from an external transaction and its receipt.
    pub fn from_external(tx: ExternalTransaction, receipt: ExternalReceipt, execution: EvmExecutionResult) -> Self {
        Self {
            kind: TransactionKind::new_external(tx, receipt),
            result: execution,
        }
    }

    /// Returns the transaction hash.
    pub fn hash(&self) -> Hash {
        match self.kind {
            TransactionKind::Local(ref tx) => tx.hash,
            TransactionKind::External(ref tx, _) => tx.hash(),
        }
    }
}
