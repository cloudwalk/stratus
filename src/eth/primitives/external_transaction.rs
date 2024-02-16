use ethers_core::types::Transaction as EthersTransaction;

use crate::eth::primitives::transaction_input::ConversionError;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Hash;
use crate::eth::primitives::TransactionInput;

#[derive(Debug, Clone, Default, derive_more:: Deref, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalTransaction(#[deref] pub EthersTransaction);

impl ExternalTransaction {
    /// Returns the transaction hash.
    pub fn hash(&self) -> Hash {
        self.0.hash.into()
    }

    /// Returns the gas limit according to the transaction type.
    pub fn gas_limit(&self, receipt: &ExternalReceipt) -> Gas {
        // when external transaction is success, use max gas to avoid failing locally due to different gas calculation
        if receipt.is_success() {
            return Gas::MAX;
        }

        // in case of failure, try to reproduce the same original behavior using the original gas
        self.0.gas.into()
    }
}

impl TryFrom<ExternalTransaction> for TransactionInput {
    type Error = ConversionError;
    fn try_from(value: ExternalTransaction) -> Result<Self, Self::Error> {
        value.0.try_into()
    }
}

impl From<EthersTransaction> for ExternalTransaction {
    fn from(value: EthersTransaction) -> Self {
        ExternalTransaction(value)
    }
}
