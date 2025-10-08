use crate::alias::AlloyReceipt;
use crate::alias::AlloyTransaction;
use crate::alias::JsonValue;
use crate::eth::executor::EvmExecutionResult;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionMined;
use crate::eth::primitives::transaction_mined::MinedData;
use crate::ext::to_json_value;

pub enum TransactionStage {
    Pending(TransactionExecution),
    Mined(TransactionMined),
}

impl TransactionStage {
    pub fn to_json_rpc_receipt(self) -> JsonValue {
        to_json_value(AlloyReceipt::from(self))
    }

    pub fn to_json_rpc_transaction(self) -> JsonValue {
        to_json_value(AlloyTransaction::from(self))
    }

    pub fn to_result(self) -> EvmExecutionResult {
        match self {
            TransactionStage::Mined(tx) => tx.execution.result,
            TransactionStage::Pending(tx) => tx.result,
        }
    }
}

impl From<TransactionStage> for (TransactionExecution, Option<MinedData>) {
    fn from(value: TransactionStage) -> Self {
        match value {
            TransactionStage::Mined(tx) => (tx.execution, Some(tx.mined_data)),
            TransactionStage::Pending(tx) => (tx, None),
        }
    }
}

impl From<TransactionStage> for AlloyReceipt {
    fn from(value: TransactionStage) -> Self {
        match value {
            TransactionStage::Mined(tx) => tx.into(),
            TransactionStage::Pending(tx) => tx.into(),
        }
    }
}

impl From<TransactionStage> for AlloyTransaction {
    fn from(value: TransactionStage) -> Self {
        match value {
            TransactionStage::Mined(tx) => tx.into(),
            TransactionStage::Pending(tx) => tx.into(),
        }
    }
}
