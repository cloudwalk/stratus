use alloy_consensus::Signed;
use alloy_consensus::TxEnvelope;
use alloy_consensus::TxLegacy;
use alloy_primitives::Bytes;
use alloy_primitives::PrimitiveSignature;
use alloy_primitives::TxKind;
use anyhow::Context;
use anyhow::Result;
use ethereum_types::U256;
use fake::Dummy;
use fake::Fake;
use fake::Faker;

use crate::alias::AlloyTransaction;
use crate::eth::primitives::signature_component::SignatureComponent;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Wei;

#[derive(Debug, Clone, PartialEq, derive_more::Deref, serde::Deserialize, serde::Serialize)]
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

impl Dummy<Faker> for ExternalTransaction {
    fn dummy_with_rng<R: rand_core::RngCore + ?Sized>(faker: &Faker, rng: &mut R) -> Self {
        let from: Address = faker.fake_with_rng(rng);
        let block_hash: Hash = faker.fake_with_rng(rng);

        let gas_price: Wei = Wei::from(rng.next_u64());
        let value: Wei = Wei::from(rng.next_u64());

        let tx = TxLegacy {
            chain_id: Some(1),
            nonce: rng.next_u64(),
            gas_price: gas_price.into(),
            gas_limit: rng.next_u64(),
            to: TxKind::Call(from.into()),
            value: value.into(),
            input: Bytes::default(),
        };

        let r = U256::from(rng.next_u64());
        let s = U256::from(rng.next_u64());
        let v = rng.next_u64() % 2 == 0;
        let signature = PrimitiveSignature::new(SignatureComponent(r).into(), SignatureComponent(s).into(), v);

        let hash: Hash = faker.fake_with_rng(rng);
        let inner_tx = TxEnvelope::Legacy(Signed::new_unchecked(tx, signature, hash.into()));

        let inner = alloy_rpc_types_eth::Transaction {
            inner: inner_tx,
            block_hash: Some(block_hash.into()),
            block_number: Some(rng.next_u64()),
            transaction_index: Some(rng.next_u64()),
            from: from.into(),
            effective_gas_price: Some(gas_price.as_u128()),
        };

        ExternalTransaction(inner)
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
