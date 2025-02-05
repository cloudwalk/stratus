use alloy_consensus::Signed;
use alloy_consensus::TxEnvelope;
use alloy_consensus::TxLegacy;
use alloy_primitives::Address;
use alloy_primitives::PrimitiveSignature;
use alloy_primitives::B256;
use alloy_primitives::U256;
use alloy_rpc_types_eth::Transaction;
use anyhow::Context;
use anyhow::Result;

use crate::alias::AlloyTransaction;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
#[derive(Debug, Clone, derive_more::Deref, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalTransaction(#[deref] pub AlloyTransaction);

impl ExternalTransaction {
    /// Returns the block number where the transaction was mined.
    pub fn block_number(&self) -> Result<BlockNumber> {
        Ok(self.0.block_number.context("ExternalTransaction has no block_number")?.into())
    }

    /// Returns the transaction hash.
    pub fn hash(&self) -> Hash {
        Hash::from(*self.0.inner.tx_hash())
    }
}

// TODO: improve before merging(add other tx types)
impl Default for ExternalTransaction {
    fn default() -> Self {
        Self(Transaction {
            inner: TxEnvelope::Legacy(Signed::new_unchecked(
                TxLegacy::default(),
                PrimitiveSignature::new(U256::ZERO, U256::ZERO, false),
                B256::default(),
            )),
            block_hash: None,
            block_number: None,
            transaction_index: None,
            from: Address::ZERO,
            effective_gas_price: None,
        })
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl From<AlloyTransaction> for ExternalTransaction {
    fn from(value: AlloyTransaction) -> Self {
        ExternalTransaction(value)
    }
}
