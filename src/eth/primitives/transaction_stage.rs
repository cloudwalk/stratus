use ethers_core::types::Transaction as EthersTransaction;
use ethers_core::types::TransactionReceipt as EthersReceipt;
use serde_json::Value as JsonValue;

use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionMined;
use crate::ext::to_json_value;

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
                to_json_value(json_rpc_payload)
            }
            TransactionStage::Executed(TransactionExecution::External(tx)) => {
                // remove block information because we don't know to which local block the transaction will be added to.
                let mut ethers_tx = tx.tx.0;
                ethers_tx.block_number = None;
                ethers_tx.block_hash = None;
                to_json_value(ethers_tx)
            }
            TransactionStage::Mined(tx) => {
                let json_rpc_payload: EthersTransaction = tx.into();
                to_json_value(json_rpc_payload)
            }
        }
    }

    /// Serializes itself to JSON-RPC receipt format.
    pub fn to_json_rpc_receipt(self) -> JsonValue {
        match self {
            TransactionStage::Executed(_) => JsonValue::Null,
            TransactionStage::Mined(tx) => {
                let json_rpc_format: EthersReceipt = tx.into();
                to_json_value(json_rpc_format)
            }
        }
    }
}
