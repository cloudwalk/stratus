use ethers_core::types::Transaction as EthersTransaction;

#[derive(Debug, Clone, Default, derive_more:: Deref, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalTransaction(#[deref] EthersTransaction);
