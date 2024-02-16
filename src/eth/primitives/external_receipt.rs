use ethers_core::types::TransactionReceipt as EthersReceipt;

use super::BlockNumber;
use crate::eth::primitives::Hash;
use crate::log_and_err;

#[derive(Debug, Clone, derive_more:: Deref, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalReceipt(#[deref] pub EthersReceipt);

impl ExternalReceipt {
    /// Returns the transaction hash.
    pub fn hash(&self) -> Hash {
        self.0.transaction_hash.into()
    }

    /// Returns the block number.
    pub fn block_number(&self) -> BlockNumber {
        self.0.block_number.expect("external receipt must have block number").into()
    }

    /// Checks if the receipt is for a transaction that was completed successfully.
    pub fn is_success(&self) -> bool {
        match self.0.status {
            Some(status) => status.as_u64() == 1,
            None => false,
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<EthersReceipt> for ExternalReceipt {
    fn from(value: EthersReceipt) -> Self {
        ExternalReceipt(value)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl TryFrom<serde_json::Value> for ExternalReceipt {
    type Error = anyhow::Error;

    fn try_from(value: serde_json::Value) -> Result<Self, Self::Error> {
        match serde_json::from_value(value.clone()) {
            Ok(v) => Ok(v),
            Err(e) => log_and_err!(reason = e, payload = value, "failed to convert payload value to ExternalBlock"),
        }
    }
}
