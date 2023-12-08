use ethereum_types::U64;
use ethers_core::types::TransactionReceipt as EthersReceipt;

use crate::eth::primitives::Hash;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct TransactionReceipt(EthersReceipt);

impl TransactionReceipt {
    /// Creates a new receipt for a executed transaction.
    pub fn confirmed(hash: Hash) -> Self {
        let receipt = EthersReceipt {
            transaction_hash: hash.into(),
            status: Some(U64::one()),
            ..Default::default()
        };
        Self(receipt)
    }
}
