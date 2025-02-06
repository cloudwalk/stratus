use anyhow::Context;
use anyhow::Result;

use crate::alias::AlloyTransaction;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
#[derive(Debug, Clone, derive_more::Deref, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalTransaction(#[deref] pub AlloyTransaction);

impl ExternalTransaction {
    /// Returns the block number where the transaction was mined.
    pub fn block_number(&self) -> Result<BlockNumber> {
        Ok(self.0.block_number.context("ExternalTransaction has no block_number")?.into())
    }

    /// Returns the transaction hash.
    pub fn hash(&self) -> Hash {
        Hash::from(*self.0.inner.tx_hash())
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl From<AlloyTransaction> for ExternalTransaction {
    fn from(value: AlloyTransaction) -> Self {
        ExternalTransaction(value)
    }
}
