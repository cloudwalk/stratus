use crate::alias::AlloyReceipt;
use crate::alias::AlloyTransaction;
use crate::alias::JsonValue;
use crate::eth::executor::TransactionExecution;
use crate::eth::executor::TransactionExecutionOutput;
use crate::eth::types::MinedData;
use crate::eth::types::TransactionMined;
use crate::ext::to_json_value;

pub enum TransactionStage {
    Pending(TransactionExecution),
    Mined(TransactionMined),
}

impl TransactionStage {
    pub fn to_json_rpc_receipt(self) -> JsonValue {
        match self {
            TransactionStage::Mined(tx) => to_json_value(AlloyReceipt::from(tx)),
            TransactionStage::Pending(_) => JsonValue::Null,
        }
    }

    pub fn to_json_rpc_transaction(self) -> JsonValue {
        to_json_value(AlloyTransaction::from(self))
    }

    pub fn to_result(self) -> TransactionExecutionOutput {
        match self {
            TransactionStage::Mined(tx) => tx.execution.output,
            TransactionStage::Pending(tx) => tx.output,
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
