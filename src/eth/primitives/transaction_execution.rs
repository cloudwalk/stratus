use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::TransactionKind;

#[derive(Debug, Clone)]
pub struct TransactionExecution {
    pub kind: TransactionKind,
    pub execution: EvmExecution,
}

impl TransactionExecution {
    pub fn new_external(transaction: ExternalTransaction, receipt: ExternalReceipt, evm_execution: EvmExecution) -> Self {
        Self {
            kind: TransactionKind::new_external(transaction, receipt),
            execution: evm_execution,
        }
    }
}
