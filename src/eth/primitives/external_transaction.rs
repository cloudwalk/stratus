use ethers_core::types::Transaction as EthersTransaction;

use crate::eth::primitives::Execution;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::Hash;
use crate::eth::primitives::TransactionInput;

pub type ExternalTransactionExecution = (ExternalTransaction, ExternalReceipt, Execution);

#[derive(Debug, Clone, Default, derive_more:: Deref, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalTransaction(#[deref] pub EthersTransaction);

impl ExternalTransaction {
    /// Returns the transaction hash.
    pub fn hash(&self) -> Hash {
        self.0.hash.into()
    }
}

impl TryFrom<ExternalTransaction> for TransactionInput {
    type Error = anyhow::Error;
    fn try_from(value: ExternalTransaction) -> Result<Self, Self::Error> {
        value.0.try_into()
    }
}

impl From<EthersTransaction> for ExternalTransaction {
    fn from(value: EthersTransaction) -> Self {
        ExternalTransaction(value)
    }
}
