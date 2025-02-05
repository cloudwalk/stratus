use alloy_consensus::Signed;
use alloy_consensus::Transaction;
use alloy_consensus::TxEnvelope;
use alloy_consensus::TxLegacy;
use alloy_eips::eip2718::Decodable2718;
use alloy_primitives::PrimitiveSignature;
use alloy_primitives::TxKind;
use anyhow::anyhow;
use display_json::DebugAsJson;
use ethereum_types::U256;
use ethereum_types::U64;
use fake::Dummy;
use fake::Fake;
use fake::Faker;
use rlp::Decodable;

use crate::alias::AlloyTransaction;
use crate::eth::primitives::signature_component::SignatureComponent;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ChainId;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Wei;

#[derive(DebugAsJson, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TransactionInput {
    /// This is needed for relaying transactions correctly, a transaction sent as Legacy should
    /// be relayed using rlp to the legacy format, the same is true for the other possible formats.
    /// Otherwise we'd need to re-sign the transactions to always encode in the same format.
    pub tx_type: Option<U64>,
    /// TODO: Optional for external/older transactions, but it should be required for newer transactions.
    ///
    /// Maybe TransactionInput should be split into two structs for representing these two different requirements.
    pub chain_id: Option<ChainId>,
    pub hash: Hash,
    pub nonce: Nonce,
    pub signer: Address,
    pub from: Address,
    pub to: Option<Address>,
    pub value: Wei,
    pub input: Bytes,
    pub gas_limit: Gas,
    pub gas_price: Wei,

    pub v: U64,
    pub r: U256,
    pub s: U256,
}

impl Dummy<Faker> for TransactionInput {
    fn dummy_with_rng<R: rand_core::RngCore + ?Sized>(faker: &Faker, rng: &mut R) -> Self {
        Self {
            tx_type: Some(rng.next_u64().into()),
            chain_id: faker.fake_with_rng(rng),
            hash: faker.fake_with_rng(rng),
            nonce: faker.fake_with_rng(rng),
            signer: faker.fake_with_rng(rng),
            from: faker.fake_with_rng(rng),
            to: Some(faker.fake_with_rng(rng)),
            value: faker.fake_with_rng(rng),
            input: faker.fake_with_rng(rng),
            gas_limit: faker.fake_with_rng(rng),
            gas_price: faker.fake_with_rng(rng),
            v: rng.next_u64().into(),
            r: rng.next_u64().into(),
            s: rng.next_u64().into(),
        }
    }
}

// -----------------------------------------------------------------------------
// Serialization / Deserialization
// -----------------------------------------------------------------------------
// TODO: improve before merging
impl Decodable for TransactionInput {
    fn decode(rlp: &rlp::Rlp) -> Result<Self, rlp::DecoderError> {
        // Decode the raw bytes using decode_2718
        let raw_bytes = rlp.as_raw();
        let envelope =
            alloy_consensus::TxEnvelope::decode_2718(&mut &raw_bytes[..]).map_err(|_| rlp::DecoderError::Custom("failed to decode transaction envelope"))?;

        match Self::try_from(alloy_rpc_types_eth::Transaction {
            inner: envelope,
            block_hash: None,
            block_number: None,
            transaction_index: None,
            from: Address::default().into(),
            effective_gas_price: None,
        }) {
            Ok(transaction) => Ok(transaction),
            Err(_) => Err(rlp::DecoderError::Custom("failed to convert transaction")),
        }
    }
}

// -----------------------------------------------------------------------------
// Conversion: Other -> Self
// -----------------------------------------------------------------------------
impl TryFrom<ExternalTransaction> for TransactionInput {
    type Error = anyhow::Error;

    fn try_from(value: ExternalTransaction) -> anyhow::Result<Self> {
        try_from_alloy_transaction(value.0, false)
    }
}

impl TryFrom<AlloyTransaction> for TransactionInput {
    type Error = anyhow::Error;

    fn try_from(value: AlloyTransaction) -> anyhow::Result<Self> {
        try_from_alloy_transaction(value, true)
    }
}

// TODO: improve before merging
fn try_from_alloy_transaction(value: alloy_rpc_types_eth::Transaction, compute_signer: bool) -> anyhow::Result<TransactionInput> {
    // extract signer
    let signer: Address = match compute_signer {
        true => match value.inner.recover_signer() {
            Ok(signer) => Address::from(signer),
            Err(e) => {
                tracing::warn!(reason = ?e, "failed to recover transaction signer");
                return Err(anyhow!("Transaction signer cannot be recovered. Check the transaction signature is valid."));
            }
        },
        false => Address::from(value.from),
    };

    // Get signature components from the envelope
    let signature = value.inner.signature();
    let r = U256::from(signature.r().to_be_bytes::<32>());
    let s = U256::from(signature.s().to_be_bytes::<32>());
    let v = if signature.v() { U64::from(1) } else { U64::from(0) };

    Ok(TransactionInput {
        tx_type: Some(U64::from(value.inner.tx_type() as u8)),
        chain_id: value.inner.chain_id().map(Into::into),
        hash: Hash::from(*value.inner.tx_hash()),
        nonce: Nonce::from(value.inner.nonce()),
        signer,
        from: Address::from(value.from),
        to: match value.inner.kind() {
            TxKind::Call(addr) => Some(Address::from(addr)),
            TxKind::Create => None,
        },
        value: Wei::from(value.inner.value()),
        input: Bytes::from(value.inner.input().clone()),
        gas_limit: Gas::from(value.inner.gas_limit()),
        gas_price: Wei::from(value.inner.gas_price().or(value.effective_gas_price).unwrap_or_default()),
        v,
        r,
        s,
    })
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<TransactionInput> for AlloyTransaction {
    fn from(value: TransactionInput) -> Self {
        let inner = TxEnvelope::Legacy(Signed::new_unchecked(
            // TODO: improve before merging - implement other types
            TxLegacy {
                chain_id: value.chain_id.map(Into::into),
                nonce: value.nonce.into(),
                gas_price: value.gas_price.into(),
                gas_limit: value.gas_limit.into(),
                to: match value.to {
                    Some(addr) => TxKind::Call(addr.into()),
                    None => TxKind::Create,
                },
                value: value.value.into(),
                input: value.input.clone().into(),
            },
            PrimitiveSignature::new(SignatureComponent(value.r).into(), SignatureComponent(value.s).into(), value.v.as_u64() == 1),
            value.hash.into(),
        ));

        Self {
            inner,
            block_hash: None,
            block_number: None,
            transaction_index: None,
            from: value.signer.into(),
            effective_gas_price: Some(value.gas_price.into()),
        }
    }
}
