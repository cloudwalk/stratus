use ethers_core::types::Transaction as EthersTransaction;

use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;

#[derive(Debug, Clone, Default, derive_more:: Deref, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalTransaction(#[deref] pub EthersTransaction);

impl ExternalTransaction {
    /// Returns the block number where the transaction was mined.
    pub fn block_number(&self) -> BlockNumber {
        self.0.block_number.unwrap().into()
    }

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
