use ethers_core::types::Transaction as EthersTransaction;
use ethers_core::types::TransactionReceipt as EthersReceipt;
use itertools::Itertools;

use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::ext::OptionExt;
use crate::if_else;

/// Transaction that was executed by the EVM and added to a block.
#[derive(Debug, Clone)]
pub struct TransactionMined {
    /// Address who signed the transaction.
    pub signer: Address,

    /// Transaction input received through RPC.
    pub input: TransactionInput,

    /// Transaction EVM execution result.
    pub execution: TransactionExecution,

    /// Logs added to the block.
    pub logs: Vec<LogMined>,

    /// Position of the transaction inside the block.
    pub transaction_index: usize,

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

    /// Convert itself into a receipt to be sent to the client.
    pub fn to_receipt(self) -> EthersReceipt {
        EthersReceipt {
            // receipt specific
            status: Some(if_else!(self.is_success(), 1, 0).into()),
            contract_address: self.execution.contract_address().map_into(),

            // transaction
            transaction_hash: self.input.hash.into(),
            from: self.input.from.into(),
            to: self.input.to.map_into(),
            gas_used: Some(self.input.gas.into()),

            // block
            block_hash: Some(self.block_hash.into()),
            block_number: Some(self.block_number.into()),
            transaction_index: self.transaction_index.into(),

            // logs
            logs: self.logs.into_iter().map_into().collect(),

            ..Default::default()
        }
    }
}

// -----------------------------------------------------------------------------
// Serialization / Deserialization
// -----------------------------------------------------------------------------
impl serde::Serialize for TransactionMined {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let trx: EthersTransaction = self.clone().into();
        trx.serialize(serializer)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<TransactionMined> for EthersTransaction {
    fn from(value: TransactionMined) -> Self {
        let input = value.input;
        EthersTransaction {
            from: value.signer.into(),
            block_hash: Some(value.block_hash.into()),
            block_number: Some(value.block_number.into()),
            transaction_index: Some(value.transaction_index.into()),
            ..input.inner
        }
    }
}
