use super::Address;
use super::BlockNumber;
use super::ExecutionResult;
use super::Index;
use crate::alias::AlloyReceipt;
use crate::alias::AlloyTransaction;
use crate::alias::JsonValue;
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
    pub fn to_json_rpc_transaction(self) -> anyhow::Result<JsonValue> {
        Ok(match self {
            TransactionStage::Executed(tx) => {
                let json_rpc_payload: AlloyTransaction = tx.input.try_into()?;
                to_json_value(json_rpc_payload)
            }
            TransactionStage::Mined(tx) => {
                let json_rpc_payload: AlloyTransaction = tx.try_into()?;
                to_json_value(json_rpc_payload)
            }
        })
    }

    /// Serializes itself to JSON-RPC receipt format.
    pub fn to_json_rpc_receipt(self) -> anyhow::Result<JsonValue> {
        Ok(match self {
            TransactionStage::Executed(_) => JsonValue::Null,
            TransactionStage::Mined(tx) => {
                let json_rpc_format: AlloyReceipt = tx.try_into()?;
                to_json_value(json_rpc_format)
            }
        })
    }

    pub fn deployed_contract_address(&self) -> Option<Address> {
        match self {
            Self::Executed(tx) => tx.result.execution.deployed_contract_address,
            Self::Mined(tx) => tx.execution.deployed_contract_address,
        }
    }

    pub fn result(&self) -> &ExecutionResult {
        match self {
            Self::Executed(tx) => &tx.result.execution.result,
            Self::Mined(tx) => &tx.execution.result,
        }
    }

    pub fn index(&self) -> Option<Index> {
        match self {
            Self::Mined(tx) => Some(tx.transaction_index),
            _ => None,
        }
    }

    pub fn block_number(&self) -> BlockNumber {
        match self {
            Self::Executed(tx) => tx.evm_input.block_number,
            Self::Mined(tx) => tx.block_number,
        }
    }

    pub fn from(&self) -> Address {
        match self {
            Self::Executed(tx) => tx.evm_input.from,
            Self::Mined(tx) => tx.input.signer,
        }
    }

    pub fn to(&self) -> Option<Address> {
        match self {
            Self::Executed(tx) => tx.evm_input.to,
            Self::Mined(tx) => tx.input.to,
        }
    }
}
