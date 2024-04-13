use std::collections::HashMap;

use anyhow::anyhow;
use itertools::Itertools;

use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::Hash;

/// A collection of [`ExternalReceipt`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExternalReceipts(HashMap<Hash, ExternalReceipt>);

impl ExternalReceipts {
    /// Generates a new collection of receipts only with receipts of the specified block number.
    pub fn filter_block(&self, number: BlockNumber) -> ExternalReceipts {
        let receipts = self.0.values().filter(|receipt| receipt.block_number() == number).cloned().collect_vec();
        ExternalReceipts::from(receipts)
    }

    /// Tries to take a receipt by its hash.
    pub fn try_get(&self, hash: &Hash) -> anyhow::Result<&ExternalReceipt> {
        match self.get(hash) {
            Some(receipt) => Ok(receipt),
            None => {
                tracing::error!(%hash, "receipt is missing for hash");
                Err(anyhow!("receipt missing for hash {}", hash))
            }
        }
    }

    /// Takes a receipt by its hash.
    pub fn get(&self, hash: &Hash) -> Option<&ExternalReceipt> {
        self.0.get(hash)
    }

    /// Returns the number of receipts.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if not contains any receipts.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<Vec<ExternalReceipt>> for ExternalReceipts {
    fn from(receipts: Vec<ExternalReceipt>) -> Self {
        let mut receipts_by_hash = HashMap::with_capacity(receipts.len());
        for receipt in receipts {
            receipts_by_hash.insert(receipt.hash(), receipt);
        }
        Self(receipts_by_hash)
    }
}
