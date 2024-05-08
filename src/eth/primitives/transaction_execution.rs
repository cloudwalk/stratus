use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::TransactionKind;

#[derive(Debug, Clone)]
pub struct TransactionExecution {
    pub kind: TransactionKind,
    pub execution: EvmExecution,
}

impl TransactionExecution {
    /// Creates a new transaction execution from a local transaction.
    pub fn new_local(transaction: TransactionInput, execution: EvmExecution) -> Self {
        Self {
            kind: TransactionKind::new_local(transaction),
            execution,
        }
    }

    /// Creates a new transaction execution from an external transaction and its receipt.
    pub fn new_external(transaction: ExternalTransaction, receipt: ExternalReceipt, execution: EvmExecution) -> Self {
        Self {
            kind: TransactionKind::new_external(transaction, receipt),
            execution,
        }
    }
}
