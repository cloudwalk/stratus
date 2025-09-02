use std::collections::HashMap;

use anyhow::anyhow;
use fake::Dummy;
use fake::Faker;

use crate::eth::primitives::ExternalReceipt;
use crate::eth::primitives::Hash;

/// A collection of [`ExternalReceipt`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExternalReceipts(HashMap<Hash, ExternalReceipt>);

impl ExternalReceipts {
    /// Tries to remove a receipt by its hash.
    pub fn try_remove(&mut self, tx_hash: Hash) -> anyhow::Result<ExternalReceipt> {
        match self.0.remove(&tx_hash) {
            Some(receipt) => Ok(receipt),
            None => {
                tracing::error!(%tx_hash, "receipt is missing for hash");
                Err(anyhow!("receipt missing for hash {}", tx_hash))
            }
        }
    }
}

impl Dummy<Faker> for ExternalReceipts {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(faker: &Faker, rng: &mut R) -> Self {
        let count = (rng.next_u32() % 5 + 1) as usize;
        let receipts = (0..count).map(|_| ExternalReceipt::dummy_with_rng(faker, rng)).collect::<Vec<_>>();

        Self::from(receipts)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl From<Vec<ExternalReceipt>> for ExternalReceipts {
    fn from(receipts: Vec<ExternalReceipt>) -> Self {
        let mut receipts_by_hash = HashMap::with_capacity(receipts.len());
        for receipt in receipts {
            receipts_by_hash.insert(receipt.hash(), receipt);
        }
        Self(receipts_by_hash)
    }
}
