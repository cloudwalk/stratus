use ethers_core::types::Transaction as EthersTransaction;

use crate::eth::primitives::transaction_input::ConversionError;
use crate::eth::primitives::TransactionInput;

#[derive(Debug, Clone, Default, derive_more:: Deref, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalTransaction(#[deref] EthersTransaction);

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
