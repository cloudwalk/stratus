use super::Address;
use super::BlockNumber;
use super::ExecutionResult;
use super::Index;
use crate::alias::AlloyReceipt;
use crate::alias::EthersTransaction;
use crate::alias::JsonValue;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionMined;
use crate::ext::to_json_value;
use crate::ext::OptionExt;

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
                let json_rpc_format: AlloyReceipt = tx.into();
                to_json_value(json_rpc_format)
            }
        }
    }

    pub fn result(&self) -> &ExecutionResult {
        match self {
            Self::Executed(tx) => &tx.result().execution.result,
            Self::Mined(tx) => &tx.execution.result,
        }
    }

    pub fn index(&self) -> Option<Index> {
        match self {
            Self::Executed(TransactionExecution::External(tx)) => tx.receipt.transaction_index.map(Index::from),
            Self::Mined(tx) => Some(tx.transaction_index),
            _ => None,
        }
    }

    pub fn block_number(&self) -> BlockNumber {
        match self {
            Self::Executed(TransactionExecution::External(tx)) => tx.receipt.block_number(),
            Self::Executed(TransactionExecution::Local(tx)) => tx.evm_input.block_number,
            Self::Mined(tx) => tx.block_number,
        }
    }

    pub fn from(&self) -> Address {
        match self {
            Self::Executed(TransactionExecution::External(tx)) => tx.receipt.from.into(),
            Self::Executed(TransactionExecution::Local(tx)) => tx.evm_input.from,
            Self::Mined(tx) => tx.input.from,
        }
    }

    pub fn to(&self) -> Option<Address> {
        match self {
            Self::Executed(TransactionExecution::External(tx)) => tx.receipt.to.map_into(),
            Self::Executed(TransactionExecution::Local(tx)) => tx.evm_input.to,
            Self::Mined(tx) => tx.input.to,
        }
    }
}
