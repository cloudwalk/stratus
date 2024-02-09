use ethers_core::types::Block as EthersBlock;

use super::BlockNumber;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::Hash;
use crate::log_and_err;

#[derive(Debug, Clone, derive_more:: Deref, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalBlock(#[deref] pub EthersBlock<ExternalTransaction>);

impl ExternalBlock {
    /// Returns the block hash.
    pub fn hash(&self) -> Hash {
        self.0.hash.expect("external block must have hash").into()
    }

    /// Returns the block number.
    pub fn number(&self) -> BlockNumber {
        self.0.number.expect("external block must have number").into()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl TryFrom<serde_json::Value> for ExternalBlock {
    type Error = anyhow::Error;

    fn try_from(value: serde_json::Value) -> Result<Self, Self::Error> {
        match serde_json::from_value(value.clone()) {
            Ok(v) => Ok(v),
            Err(e) => log_and_err!(reason = e, payload = value, "failed to convert payload value to ExternalBlock"),
        }
    }
}
