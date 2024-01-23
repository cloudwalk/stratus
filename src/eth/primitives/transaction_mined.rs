//! Transaction Mined Module
//!
//! Manages Ethereum transactions that have been executed and included in a
//! mined block. This module extends transaction information to include
//! blockchain context such as block number and hash. It is crucial for
//! tracking transaction history and understanding the state of transactions in
//! the blockchain.

use ethers_core::types::Transaction as EthersTransaction;
use ethers_core::types::TransactionReceipt as EthersReceipt;
use itertools::Itertools;
use serde_json::Value as JsonValue;

use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::ext::OptionExt;
use crate::if_else;

/// Transaction that was executed by the EVM and added to a block.
#[derive(Debug, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct TransactionMined {
    /// Transaction input received through RPC.
    pub input: TransactionInput,

    /// Transaction EVM execution result.
    pub execution: TransactionExecution,

    /// Logs added to the block.
    pub logs: Vec<LogMined>,

    /// Position of the transaction inside the block.
    pub transaction_index: Index,

    /// Block number where the transaction was mined.
    pub block_number: BlockNumber,

    /// Block hash where the transaction was mined.
    pub block_hash: Hash,
}

impl TransactionMined {
    /// Check if the current transaction was completed normally.
    pub fn is_success(&self) -> bool {
        self.execution.is_success()
    }

    /// Serializes itself to JSON-RPC transaction format.
    pub fn to_json_rpc_transaction(self) -> JsonValue {
        let json_rpc_format: EthersTransaction = self.into();
        serde_json::to_value(json_rpc_format).unwrap()
    }

    /// Serializes itself to JSON-RPC receipt format.
    pub fn to_json_rpc_receipt(self) -> JsonValue {
        let json_rpc_format: EthersReceipt = self.into();
        serde_json::to_value(json_rpc_format).unwrap()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<TransactionMined> for EthersTransaction {
    fn from(value: TransactionMined) -> Self {
        let input = value.input;
        Self {
            chain_id: Some(input.chain_id.into()),
            hash: input.hash.into(),
            nonce: input.nonce.into(),
            block_hash: Some(value.block_hash.into()),
            block_number: Some(value.block_number.into()),
            transaction_index: Some(value.transaction_index.into()),
            from: input.signer.into(),
            to: input.to.map_into(),
            value: input.value.into(),
            gas_price: Some(input.gas_price.into()),
            gas: input.gas.into(),
            input: input.input.into(),
            v: input.v,
            r: input.r,
            s: input.s,
            ..Default::default()
        }
    }
}

impl From<TransactionMined> for EthersReceipt {
    fn from(value: TransactionMined) -> Self {
        Self {
            // receipt specific
            status: Some(if_else!(value.is_success(), 1, 0).into()),
            contract_address: value.execution.contract_address().map_into(),

            // transaction
            transaction_hash: value.input.hash.into(),
            from: value.input.signer.into(),
            to: value.input.to.map_into(),
            gas_used: Some(value.input.gas.into()),

            // block
            block_hash: Some(value.block_hash.into()),
            block_number: Some(value.block_number.into()),
            transaction_index: value.transaction_index.into(),

            // logs
            logs: value.logs.into_iter().map_into().collect(),

            // TODO: there are more fields to populate here
            ..Default::default()
        }
    }
}
