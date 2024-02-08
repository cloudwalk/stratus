use ethers_core::types::Block as EthersBlock;

use crate::eth::primitives::ExternalTransaction;

#[derive(Debug, Clone, derive_more:: Deref, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalBlock(#[deref] EthersBlock<ExternalTransaction>);
