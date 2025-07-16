use alloy_consensus::Signed;
use alloy_consensus::Transaction;
use alloy_consensus::TxEip1559;
use alloy_consensus::TxEip2930;
use alloy_consensus::TxEip4844;
use alloy_consensus::TxEip4844Variant;
use alloy_consensus::TxEip7702;
use alloy_consensus::TxEnvelope;
use alloy_consensus::TxLegacy;
use alloy_consensus::transaction::Recovered;
use alloy_eips::eip2718::Decodable2718;
use alloy_primitives::Signature;
use alloy_primitives::TxKind;
use alloy_primitives::U64;
use alloy_primitives::U256;
use alloy_rpc_types_eth::AccessList;
use anyhow::anyhow;
use display_json::DebugAsJson;
use fake::Dummy;
use fake::Fake;
use fake::Faker;
use rlp::Decodable;

use crate::alias::AlloyTransaction;
use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ChainId;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Wei;
use crate::eth::primitives::signature_component::SignatureComponent;
use crate::ext::RuintExt;

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
    pub gas_price: u128,

    pub v: U64,
    pub r: U256,
    pub s: U256,
}

impl Dummy<Faker> for TransactionInput {
    fn dummy_with_rng<R: rand::Rng + ?Sized>(faker: &Faker, rng: &mut R) -> Self {
        Self {
            tx_type: Some(U64::random_with(rng)),
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
            v: U64::random_with(rng),
            r: U256::random_with(rng),
            s: U256::random_with(rng),
        }
    }
}

// -----------------------------------------------------------------------------
// Serialization / Deserialization
// -----------------------------------------------------------------------------

impl Decodable for TransactionInput {
    fn decode(rlp: &rlp::Rlp) -> Result<Self, rlp::DecoderError> {
        fn convert_tx(envelope: TxEnvelope) -> Result<TransactionInput, rlp::DecoderError> {
            TransactionInput::try_from(alloy_rpc_types_eth::Transaction {
                inner: envelope.try_into_recovered().map_err(|_| rlp::DecoderError::Custom("signature error"))?,
                block_hash: None,
                block_number: None,
                transaction_index: None,
                effective_gas_price: None,
            })
            .map_err(|_| rlp::DecoderError::Custom("failed to convert transaction"))
        }

        let raw_bytes = rlp.as_raw();

        if raw_bytes.is_empty() {
            return Err(rlp::DecoderError::Custom("empty transaction bytes"));
        }

        if rlp.is_list() {
            // Legacy transaction
            let mut bytes = raw_bytes;
            TxEnvelope::fallback_decode(&mut bytes)
                .map_err(|_| rlp::DecoderError::Custom("failed to decode legacy transaction"))
                .and_then(convert_tx)
        } else {
            // Typed transaction (EIP-2718)
            let first_byte = raw_bytes[0];
            let mut remaining_bytes = &raw_bytes[1..];
            TxEnvelope::typed_decode(first_byte, &mut remaining_bytes)
                .map_err(|_| rlp::DecoderError::Custom("failed to decode transaction envelope"))
                .and_then(convert_tx)
        }
    }
}

// -----------------------------------------------------------------------------
// Conversion: Other -> Self
// -----------------------------------------------------------------------------
impl TryFrom<ExternalTransaction> for TransactionInput {
    type Error = anyhow::Error;

    fn try_from(value: ExternalTransaction) -> anyhow::Result<Self> {
        try_from_alloy_transaction(value.0)
    }
}

impl TryFrom<AlloyTransaction> for TransactionInput {
    type Error = anyhow::Error;

    fn try_from(value: AlloyTransaction) -> anyhow::Result<Self> {
        try_from_alloy_transaction(value)
    }
}

fn try_from_alloy_transaction(value: alloy_rpc_types_eth::Transaction) -> anyhow::Result<TransactionInput> {
    // extract signer
    let signer: Address = match value.inner.recover_signer() {
        Ok(signer) => Address::from(signer),
        Err(e) => {
            tracing::warn!(reason = ?e, "failed to recover transaction signer");
            return Err(anyhow!("Transaction signer cannot be recovered. Check the transaction signature is valid."));
        }
    };

    // Get signature components from the envelope
    let signature = value.inner.signature();
    let r = signature.r();
    let s = signature.s();
    let v = if signature.v() { U64::ONE } else { U64::ZERO };

    Ok(TransactionInput {
        tx_type: Some(U64::from(value.inner.tx_type() as u8)),
        chain_id: value.inner.chain_id().map(Into::into),
        hash: Hash::from(*value.inner.tx_hash()),
        nonce: Nonce::from(value.inner.nonce()),
        signer,
        from: signer,
        to: match value.inner.kind() {
            TxKind::Call(addr) => Some(Address::from(addr)),
            TxKind::Create => None,
        },
        value: Wei::from(value.inner.value()),
        input: Bytes::from(value.inner.input().clone()),
        gas_limit: Gas::from(value.inner.gas_limit()),
        gas_price: value.inner.gas_price().or(value.effective_gas_price).unwrap_or_default(),
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
        let signature = Signature::new(SignatureComponent(value.r).into(), SignatureComponent(value.s).into(), value.v == U64::ONE);

        let tx_type = value.tx_type.map(|t| t.as_u64()).unwrap_or(0);

        let inner = match tx_type {
            // EIP-2930
            1 => TxEnvelope::Eip2930(Signed::new_unchecked(
                TxEip2930 {
                    chain_id: value.chain_id.unwrap_or_default().into(),
                    nonce: value.nonce.into(),
                    gas_price: value.gas_price,
                    gas_limit: value.gas_limit.into(),
                    to: TxKind::from(value.to.map(Into::into)),
                    value: value.value.into(),
                    input: value.input.clone().into(),
                    access_list: AccessList::default(),
                },
                signature,
                value.hash.into(),
            )),

            // EIP-1559
            2 => TxEnvelope::Eip1559(Signed::new_unchecked(
                TxEip1559 {
                    chain_id: value.chain_id.unwrap_or_default().into(),
                    nonce: value.nonce.into(),
                    max_fee_per_gas: value.gas_price,
                    max_priority_fee_per_gas: value.gas_price,
                    gas_limit: value.gas_limit.into(),
                    to: TxKind::from(value.to.map(Into::into)),
                    value: value.value.into(),
                    input: value.input.clone().into(),
                    access_list: AccessList::default(),
                },
                signature,
                value.hash.into(),
            )),

            // EIP-4844
            3 => TxEnvelope::Eip4844(Signed::new_unchecked(
                TxEip4844Variant::TxEip4844(TxEip4844 {
                    chain_id: value.chain_id.unwrap_or_default().into(),
                    nonce: value.nonce.into(),
                    max_fee_per_gas: value.gas_price,
                    max_priority_fee_per_gas: value.gas_price,
                    gas_limit: value.gas_limit.into(),
                    to: value.to.map(Into::into).unwrap_or_default(),
                    value: value.value.into(),
                    input: value.input.clone().into(),
                    access_list: AccessList::default(),
                    blob_versioned_hashes: Vec::new(),
                    max_fee_per_blob_gas: 0u64.into(),
                }),
                signature,
                value.hash.into(),
            )),

            // EIP-7702
            4 => TxEnvelope::Eip7702(Signed::new_unchecked(
                TxEip7702 {
                    chain_id: value.chain_id.unwrap_or_default().into(),
                    nonce: value.nonce.into(),
                    gas_limit: value.gas_limit.into(),
                    max_fee_per_gas: value.gas_price,
                    max_priority_fee_per_gas: value.gas_price,
                    to: value.to.map(Into::into).unwrap_or_default(),
                    value: value.value.into(),
                    input: value.input.clone().into(),
                    access_list: AccessList::default(),
                    authorization_list: Vec::new(),
                },
                signature,
                value.hash.into(),
            )),

            // Legacy (default)
            _ => TxEnvelope::Legacy(Signed::new_unchecked(
                TxLegacy {
                    chain_id: value.chain_id.map(Into::into),
                    nonce: value.nonce.into(),
                    gas_price: value.gas_price,
                    gas_limit: value.gas_limit.into(),
                    to: TxKind::from(value.to.map(Into::into)),
                    value: value.value.into(),
                    input: value.input.clone().into(),
                },
                signature,
                value.hash.into(),
            )),
        };

        Self {
            inner: Recovered::new_unchecked(inner, value.signer.into()),
            block_hash: None,
            block_number: None,
            transaction_index: None,
            effective_gas_price: Some(value.gas_price),
        }
    }
}
