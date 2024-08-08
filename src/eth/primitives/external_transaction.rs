use anyhow::Context;
use anyhow::Result;

use crate::alias::EthersTransaction;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;

#[derive(Debug, Clone, Default, derive_more::Deref, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalTransaction(#[deref] pub EthersTransaction);

impl ExternalTransaction {
    /// Returns the block number where the transaction was mined.
    pub fn block_number(&self) -> Result<BlockNumber> {
        Ok(self.0.block_number.context("ExternalTransaction has no block_number")?.into())
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
