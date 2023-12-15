use ethers_core::types::Transaction as EthersTransaction;

use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::TransactionExecution;
use crate::eth::primitives::TransactionInput;
use crate::eth::primitives::TransactionReceipt;

/// Complete transaction after being executed by EVM and added to a block.
#[derive(Debug, Clone, derive_new::new)]
pub struct TransactionMined {
    /// Transaction input.
    pub input: TransactionInput,

    /// Transaction EVM execution result.
    pub execution: TransactionExecution,

    /// Position of the transaction inside the block.
    pub index_in_block: usize,

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
        let input = value.input;

        // create inner with block information
        EthersTransaction {
            block_hash: Some(value.block_hash.into()),
            block_number: Some(value.block_number.into()),
            transaction_index: Some(0.into()),
            ..input.inner
        }
    }
}
