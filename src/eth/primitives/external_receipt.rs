use ethereum_types::U256;
use serde::Deserialize;

use crate::alias::AlloyReceipt;
use crate::alias::JsonValue;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Wei;
use crate::log_and_err;

#[derive(Debug, Clone, derive_more::Deref, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalReceipt(#[deref] pub AlloyReceipt);

impl ExternalReceipt {
    /// Returns the transaction hash.
    pub fn hash(&self) -> Hash {
        Hash::from(self.0.transaction_hash.0)
    }

    /// Returns the block number.
    #[allow(clippy::expect_used)]
    pub fn block_number(&self) -> BlockNumber {
        self.0.block_number.expect("external receipt must have block number").into()
    }

    /// Returns the block hash.
    #[allow(clippy::expect_used)]
    pub fn block_hash(&self) -> Hash {
        Hash::from(self.0.block_hash.expect("external receipt must have block hash").0)
    }

    /// Retuns the effective price the sender had to pay to execute the transaction.
    pub fn execution_cost(&self) -> Wei {
        let gas_price = U256::from(self.0.effective_gas_price);
        let gas_used = U256::from(self.0.gas_used);
        (gas_price * gas_used).into()
    }

    /// Checks if the transaction was completed with success.
    pub fn is_success(&self) -> bool {
        self.0.inner.status()
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl TryFrom<JsonValue> for ExternalReceipt {
    type Error = anyhow::Error;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        match ExternalReceipt::deserialize(&value) {
            Ok(v) => Ok(v),
            Err(e) => log_and_err!(reason = e, payload = value, "failed to convert payload value to ExternalBlock"),
        }
    }
}
