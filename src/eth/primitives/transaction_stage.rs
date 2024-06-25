use ethers_core::types::Transaction as EthersTransaction;
use ethers_core::types::TransactionReceipt as EthersReceipt;
use serde_json::Value as JsonValue;

use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionMined;
use crate::ext::ResultExt;

/// Stages that a transaction can be in.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, derive_new::new)]
pub enum TransactionStage {
    /// Transaction was executed, but is awaiting to be mined to a block.
    Executed(TransactionExecution),

    /// Transaction that was added to a mined block.
    Mined(TransactionMined),
}

impl TransactionStage {
    /// Serializes itself to JSON-RPC transaction format.
    pub fn to_json_rpc_transaction(self) -> JsonValue {
        match self {
            TransactionStage::Executed(TransactionExecution::Local(tx)) => {
                let json_rpc_payload: EthersTransaction = tx.input.into();
                serde_json::to_value(json_rpc_payload).expect_infallible()
            }
            TransactionStage::Executed(TransactionExecution::External(tx)) => {
                // remove block information because we don't know to which local block the transaction will be added to.
                let mut ethers_tx = tx.tx.0;
                ethers_tx.block_number = None;
                ethers_tx.block_hash = None;
                serde_json::to_value(ethers_tx).expect_infallible()
            }
            TransactionStage::Mined(tx) => {
                let json_rpc_payload: EthersTransaction = tx.into();
                serde_json::to_value(json_rpc_payload).expect_infallible()
            }
        }
    }

    /// Serializes itself to JSON-RPC receipt format.
    pub fn to_json_rpc_receipt(self) -> JsonValue {
        match self {
            TransactionStage::Executed(_) => JsonValue::Null,
            TransactionStage::Mined(tx) => {
                let json_rpc_format: EthersReceipt = tx.into();
                serde_json::to_value(json_rpc_format).expect_infallible()
            }
        }
    }
}
