use ethers_core::types::TransactionReceipt as EthersReceipt;

#[derive(Debug, Clone, derive_more:: Deref, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalReceipt(#[deref] EthersReceipt);
