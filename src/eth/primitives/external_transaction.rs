use ethers_core::types::Transaction as EthersTransaction;

use crate::eth::primitives::EvmExecution;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::Hash;

#[derive(Debug, derive_new::new)]
pub struct ExternalTransactionExecution {
    pub tx: ExternalTransaction,
    pub receipt: ExternalReceipt,
    pub execution: EvmExecution,
}

#[derive(Debug, Clone, Default, derive_more:: Deref, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalTransaction(#[deref] pub EthersTransaction);

impl ExternalTransaction {
    /// Returns the transaction hash.
    pub fn hash(&self) -> Hash {
        self.0.hash.into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl From<EthersTransaction> for ExternalTransaction {
    fn from(value: EthersTransaction) -> Self {
        ExternalTransaction(value)
    }
}
