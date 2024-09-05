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

    /// Fills the field transaction_type based on `v`
    pub fn fill_missing_transaction_type(&mut self) {
        // Don't try overriding if it's already set
        if self.0.transaction_type.is_some() {
            return;
        }

        let v = self.0.v.as_u64();
        if [0, 1].contains(&v) {
            self.0.transaction_type = Some(2.into());
        }
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
