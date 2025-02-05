use alloy_consensus::Signed;
use alloy_consensus::TxEnvelope;
use alloy_consensus::TxLegacy;
use alloy_consensus::Typed2718;
use alloy_primitives::Address;
use alloy_primitives::FixedBytes;
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

    /// Fills the field transaction_type based on transaction envelope type
    pub fn fill_missing_transaction_type(&mut self) {
        // TODO: improve before merging
        // Don't try overriding if it's already set
        if self.0.inner.ty() != 0 {
            return;
        }

        // Check if transaction is EIP-1559 based on inner type
        if self.0.inner.is_eip1559() {
            let signature = PrimitiveSignature::new(U256::ZERO, U256::ZERO, false);

            self.0.inner = alloy_consensus::TxEnvelope::Eip1559(alloy_consensus::Signed::new_unchecked(
                alloy_consensus::TxEip1559::default(),
                signature,
                FixedBytes::default(),
            ));
        }
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
