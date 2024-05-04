use ethereum_types::U256;
use ethers_core::types::TransactionReceipt as EthersReceipt;
use serde::Deserialize;

use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Wei;
use crate::ext::not;
use crate::ext::OptionExt;
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

    /// Returns the block hash.
    pub fn block_hash(&self) -> Hash {
        self.0.block_hash.expect("external receipt must have block hash").into()
    }

    /// Retuns the effective price the sender had to pay to execute the transaction.
    pub fn execution_cost(&self) -> Wei {
        let gas_price = self.0.effective_gas_price.map_into().unwrap_or(U256::zero());
        let gas_used = self.0.gas_used.map_into().unwrap_or(U256::zero());
        (gas_price * gas_used).into()
    }

    /// Checks if the transaction was completed with success.
    pub fn is_success(&self) -> bool {
        match self.0.status {
            Some(status) => status.as_u64() == 1,
            None => false,
        }
    }

    /// Checks if the transaction was completed with error.
    pub fn is_failure(&self) -> bool {
        not(self.is_success())
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
        match ExternalReceipt::deserialize(&value) {
            Ok(v) => Ok(v),
            Err(e) => log_and_err!(reason = e, payload = value, "failed to convert payload value to ExternalBlock"),
        }
    }
}
