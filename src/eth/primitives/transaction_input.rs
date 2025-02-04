use alloy_consensus::Signed;
use alloy_consensus::Transaction;
use alloy_consensus::TxEnvelope;
use alloy_consensus::TxLegacy;
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
use crate::eth::primitives::signature::SignatureComponent;
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
    fn dummy_with_rng<R: ethers_core::rand::prelude::Rng + ?Sized>(faker: &Faker, rng: &mut R) -> Self {
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
impl Decodable for TransactionInput {
    fn decode(rlp: &rlp::Rlp) -> Result<Self, rlp::DecoderError> {
        let alloy_transaction = AlloyTransaction::decode(rlp)?;
        match Self::try_from(alloy_transaction) {
            Ok(transaction) => Ok(transaction),
            Err(_) => Err(rlp::DecoderError::Custom("decoding error")),
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

fn try_from_alloy_transaction(value: alloy_rpc_types_eth::Transaction, compute_signer: bool) -> anyhow::Result<TransactionInput> {
    // extract signer
    let signer: Address = match compute_signer {
        true => match value.inner.recover_signer() {
            Ok(signer) => signer.into(),
            Err(e) => {
                tracing::warn!(reason = ?e, "failed to recover transaction signer");
                return Err(anyhow!("Transaction signer cannot be recovered. Check the transaction signature is valid."));
            }
        },
        false => value.from.into(),
    };

    // Get signature components from the envelope
    let signature = value.inner.signature();
    let (r, s) = signature.rs();
    let v = if signature.parity() { U64::from(1) } else { U64::from(0) };

    Ok(TransactionInput {
        tx_type: Some(U64::from(value.inner.to())),
        chain_id: value.inner.chain_id().map(TryInto::try_into).transpose()?,
        hash: *value.inner.tx_hash().into(),
        nonce: value.inner.nonce().try_into()?,
        signer,
        from: Address::new(value.from.into()),
        to: match value.inner.kind() {
            TxKind::Call(addr) => Some(addr.into()),
            TxKind::Create => None,
        },
        value: value.inner.value().try_into()?,
        input: value.inner.input().clone().into(),
        gas_limit: value.inner.gas_limit().try_into()?,
        gas_price: value.effective_gas_price.unwrap_or_default().try_into()?,
        v,
        r: r.into(),
        s: s.into(),
    })
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

// TODO: improve before merging
impl From<TransactionInput> for alloy_rpc_types_eth::Transaction {
    fn from(value: TransactionInput) -> Self {
        // Create legacy transaction envelope
        let inner = TxEnvelope::Legacy(Signed::new_unchecked(
            TxLegacy {
                chain_id: value.chain_id.map(Into::into),
                nonce: value.nonce.into(),
                gas_price: value.gas_price.into(),
                gas_limit: value.gas_limit.into(),
                to: Into::<TxKind>::into(value.to.map(Into::<alloy_primitives::Address>::into).unwrap_or_default()),
                value: value.value.into(),
                input: value.input.clone().into(),
            },
            alloy_primitives::PrimitiveSignature::new(SignatureComponent(value.r).into(), SignatureComponent(value.s).into(), value.v.as_u64() == 1),
            Default::default(),
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
