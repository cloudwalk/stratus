use ethers_core::types::Transaction as EthersTransaction;

use crate::eth::primitives::Block;
use crate::eth::primitives::Execution;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::TransactionReceipt;

/// Complete transaction after being executed by EVM and added to a block.
#[derive(Debug, Clone, derive_new::new)]
pub struct TransactionMined {
    /// Transaction input.
    pub transaction_input: TransactionInput,

    /// Transaction EVM execution result.
    pub execution: Execution,

    /// Block where the transaction was mined.
    pub block: Block,
}

impl TransactionMined {
    /// Check if the current transaction was completed normally.
    pub fn is_success(&self) -> bool {
        self.execution.is_success()
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
