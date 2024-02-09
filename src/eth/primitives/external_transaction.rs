use ethers_core::types::Transaction as EthersTransaction;

use crate::eth::primitives::Hash;

#[derive(Debug, Clone, Default, derive_more:: Deref, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalTransaction(#[deref] pub EthersTransaction);

impl ExternalTransaction {
    /// Returns the block hash.
    pub fn hash(&self) -> Hash {
        self.0.hash.into()
    }
}
