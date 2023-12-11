use ethers_core::types::Transaction as EthersTransaction;

use crate::eth::primitives::Block;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionExecutionResult;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::TransactionReceipt;

/// Complete transaction after being executed by EVM and added to a block.
#[derive(Debug, Clone, derive_new::new)]
pub struct TransactionMined {
    /// Transaction input.
    pub transaction_input: TransactionInput,

    /// Transaction EVM execution result.
    pub execution: TransactionExecution,

    /// Block where the transaction was mined.
    pub block: Block,
}

impl TransactionMined {
    /// Check if the current transaction was completed normally.
    pub fn is_commited(&self) -> bool {
        matches!(self.execution.result, TransactionExecutionResult::Commited { .. })
    }

    /// Convert itself into a [`TransactionReceipt`].
    pub fn to_receipt(self) -> TransactionReceipt {
        self.into()
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
        let input = value.transaction_input;
        let block = value.block;

        // create inner with block information
        EthersTransaction {
            block_hash: Some(block.hash.into()),
            block_number: Some(block.number.into()),
            transaction_index: Some(0.into()),
            ..input.inner
        }
    }
}
