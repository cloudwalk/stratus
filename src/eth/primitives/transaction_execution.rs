use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::TransactionKind;

#[derive(Debug, Clone)]
pub struct TransactionExecution {
    pub transaction: TransactionKind,
    pub evm_execution: EvmExecution,
}

impl TransactionExecution {
    pub fn new_external(transaction: ExternalTransaction, receipt: ExternalReceipt, evm_execution: EvmExecution) -> Self {
        Self {
            transaction: TransactionKind::new_external(transaction, receipt),
            evm_execution,
        }
    }
}
