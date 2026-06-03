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
use alloy_consensus::transaction::SignerRecoverable;
use alloy_eips::eip2718::Decodable2718;
use alloy_primitives::Signature as AlloySignature;
use alloy_primitives::TxKind;
use alloy_primitives::U64;
use alloy_primitives::U256;
use alloy_rpc_types_eth::AccessList;
use anyhow::Context;
use display_json::DebugAsJson;
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
#[cfg_attr(test, derive(fake::Dummy))]
pub struct TransactionInfo {
    #[cfg_attr(test, dummy(expr = "crate::utils::test_utils::fake_option_uint()"))]
    pub tx_type: Option<U64>,
    pub hash: Hash,
}

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Signer {
    Recovered(Address),
    Unrecovered,
}

impl Default for Signer {
    fn default() -> Self {
        Self::Unrecovered
    }
}

impl Signer {
    pub fn address(&self) -> Option<Address> {
        match self {
            Self::Recovered(addr) => Some(*addr),
            Self::Unrecovered => None,
        }
    }

    pub fn is_recovered(&self) -> bool {
        matches!(self, Self::Recovered(_))
    }
}

impl std::fmt::Display for Signer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Recovered(addr) => write!(f, "{addr}"),
            Self::Unrecovered => write!(f, "<unrecovered>"),
        }
    }
}

impl std::fmt::Debug for Signer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}

#[cfg(test)]
impl fake::Dummy<fake::Faker> for Signer {
    fn dummy_with_rng<R: fake::Rng + ?Sized>(_: &fake::Faker, rng: &mut R) -> Self {
        Self::Recovered(fake::Dummy::dummy_with_rng(&fake::Faker, rng))
    }
}

#[derive(DebugAsJson, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct ExecutionInfo {
    #[cfg_attr(test, dummy(expr = "crate::utils::test_utils::fake_option::<ChainId>()"))]
    pub chain_id: Option<ChainId>,
    pub nonce: Nonce,
    pub signer: Signer,
    #[cfg_attr(test, dummy(expr = "crate::utils::test_utils::fake_option::<Address>()"))]
    pub to: Option<Address>,
    pub value: Wei,
    pub input: Bytes,
    pub gas_limit: Gas,
    pub gas_price: u128,
}

#[derive(DebugAsJson, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct Signature {
    #[cfg_attr(test, dummy(expr = "crate::utils::test_utils::fake_uint()"))]
    pub v: U64,
    #[cfg_attr(test, dummy(expr = "crate::utils::test_utils::fake_uint()"))]
    pub r: U256,
    #[cfg_attr(test, dummy(expr = "crate::utils::test_utils::fake_uint()"))]
    pub s: U256,
}

impl From<Signature> for AlloySignature {
    fn from(value: Signature) -> Self {
        AlloySignature::new(SignatureComponent(value.r).into(), SignatureComponent(value.s).into(), value.v == U64::ONE)
    }
}

#[derive(DebugAsJson, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct TransactionInput {
    pub transaction_info: TransactionInfo,
    pub execution_info: ExecutionInfo,
    pub signature: Signature,
}

impl TransactionInput {
    /// Returns the recovered signer address.
    ///
    /// # Panics
    /// Panics if the signer has not been recovered. All construction paths
    /// call `recover_signer` before returning, so this is safe in practice.
    pub fn signer(&self) -> Address {
        self.execution_info.signer.address().unwrap()
    }

    /// Recovers the signer from the original transaction envelope using ECDSA recovery.
    ///
    /// This must be called after constructing a TransactionInput to ensure the signer
    /// is derived from the same recovery path regardless of the origin.
    fn recover_signer(&mut self, envelope: &TxEnvelope) -> anyhow::Result<()> {
        let signer = envelope
            .recover_signer()
            .context("Transaction signer cannot be recovered. Check the transaction signature is valid.")?;
        self.execution_info.signer = Signer::Recovered(Address::from(signer));
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Serialization / Deserialization
// -----------------------------------------------------------------------------

impl Decodable for TransactionInput {
    fn decode(rlp: &rlp::Rlp) -> Result<Self, rlp::DecoderError> {
        fn convert_tx(envelope: TxEnvelope) -> Result<TransactionInput, rlp::DecoderError> {
            let mut tx_input = try_from_alloy_transaction(alloy_rpc_types_eth::Transaction {
                inner: Recovered::new_unchecked(envelope.clone(), alloy_primitives::Address::ZERO),
                block_hash: None,
                block_number: None,
                transaction_index: None,
                effective_gas_price: None,
            })
            .map_err(|_| rlp::DecoderError::Custom("failed to convert transaction"))?;

            tx_input
                .recover_signer(&envelope)
                .map_err(|_| rlp::DecoderError::Custom("failed to recover signer"))?;
            Ok(tx_input)
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
        let envelope = value.0.inner.inner().clone();
        let mut tx_input = try_from_alloy_transaction(value.0)?;
        tx_input.recover_signer(&envelope)?;
        Ok(tx_input)
    }
}

impl TryFrom<AlloyTransaction> for TransactionInput {
    type Error = anyhow::Error;

    fn try_from(value: AlloyTransaction) -> anyhow::Result<Self> {
        let envelope = value.inner.inner().clone();
        let mut tx_input = try_from_alloy_transaction(value)?;
        tx_input.recover_signer(&envelope)?;
        Ok(tx_input)
    }
}

fn try_from_alloy_transaction(value: alloy_rpc_types_eth::Transaction) -> anyhow::Result<TransactionInput> {
    // Get signature components from the envelope
    let signature = value.inner.signature();
    let signature = Signature {
        r: signature.r(),
        s: signature.s(),
        v: if signature.v() { U64::ONE } else { U64::ZERO },
    };

    // Build TransactionInput with an unrecovered signer.
    // The actual signer must be recovered via `recover_signer` after construction
    // to ensure leader and follower use the same ECDSA recovery path.
    Ok(TransactionInput {
        transaction_info: TransactionInfo {
            tx_type: Some(U64::from(value.inner.tx_type() as u8)),
            hash: Hash::from(*value.inner.tx_hash()),
        },
        execution_info: ExecutionInfo {
            chain_id: value.inner.chain_id().map(Into::into),
            nonce: Nonce::from(value.inner.nonce()),
            signer: Signer::Unrecovered,
            to: match value.inner.kind() {
                TxKind::Call(addr) => Some(Address::from(addr)),
                TxKind::Create => None,
            },
            value: Wei::from(value.inner.value()),
            input: Bytes::from(value.inner.input().clone()),
            gas_limit: Gas::from(value.inner.gas_limit()),
            gas_price: value.inner.max_fee_per_gas(),
        },
        signature,
    })
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<TransactionInput> for AlloyTransaction {
    fn from(value: TransactionInput) -> Self {
        let signer = value.signer();
        let signature = value.signature.into();

        let tx_type = value.transaction_info.tx_type.map(|t| t.as_u64()).unwrap_or(0);

        let inner = match tx_type {
            // EIP-2930
            1 => TxEnvelope::Eip2930(Signed::new_unchecked(
                TxEip2930 {
                    chain_id: value.execution_info.chain_id.unwrap_or_default().into(),
                    nonce: value.execution_info.nonce.into(),
                    gas_price: value.execution_info.gas_price,
                    gas_limit: value.execution_info.gas_limit.into(),
                    to: TxKind::from(value.execution_info.to.map(Into::into)),
                    value: value.execution_info.value.into(),
                    input: value.execution_info.input.clone().into(),
                    access_list: AccessList::default(),
                },
                signature,
                value.transaction_info.hash.into(),
            )),

            // EIP-1559
            2 => TxEnvelope::Eip1559(Signed::new_unchecked(
                TxEip1559 {
                    chain_id: value.execution_info.chain_id.unwrap_or_default().into(),
                    nonce: value.execution_info.nonce.into(),
                    max_fee_per_gas: value.execution_info.gas_price,
                    max_priority_fee_per_gas: value.execution_info.gas_price,
                    gas_limit: value.execution_info.gas_limit.into(),
                    to: TxKind::from(value.execution_info.to.map(Into::into)),
                    value: value.execution_info.value.into(),
                    input: value.execution_info.input.clone().into(),
                    access_list: AccessList::default(),
                },
                signature,
                value.transaction_info.hash.into(),
            )),

            // EIP-4844
            3 => TxEnvelope::Eip4844(Signed::new_unchecked(
                TxEip4844Variant::TxEip4844(TxEip4844 {
                    chain_id: value.execution_info.chain_id.unwrap_or_default().into(),
                    nonce: value.execution_info.nonce.into(),
                    max_fee_per_gas: value.execution_info.gas_price,
                    max_priority_fee_per_gas: value.execution_info.gas_price,
                    gas_limit: value.execution_info.gas_limit.into(),
                    to: value.execution_info.to.map(Into::into).unwrap_or_default(),
                    value: value.execution_info.value.into(),
                    input: value.execution_info.input.clone().into(),
                    access_list: AccessList::default(),
                    blob_versioned_hashes: Vec::new(),
                    max_fee_per_blob_gas: 0u64.into(),
                }),
                signature,
                value.transaction_info.hash.into(),
            )),

            // EIP-7702
            4 => TxEnvelope::Eip7702(Signed::new_unchecked(
                TxEip7702 {
                    chain_id: value.execution_info.chain_id.unwrap_or_default().into(),
                    nonce: value.execution_info.nonce.into(),
                    gas_limit: value.execution_info.gas_limit.into(),
                    max_fee_per_gas: value.execution_info.gas_price,
                    max_priority_fee_per_gas: value.execution_info.gas_price,
                    to: value.execution_info.to.map(Into::into).unwrap_or_default(),
                    value: value.execution_info.value.into(),
                    input: value.execution_info.input.clone().into(),
                    access_list: AccessList::default(),
                    authorization_list: Vec::new(),
                },
                signature,
                value.transaction_info.hash.into(),
            )),

            // Legacy (default)
            _ => TxEnvelope::Legacy(Signed::new_unchecked(
                TxLegacy {
                    chain_id: value.execution_info.chain_id.map(Into::into),
                    nonce: value.execution_info.nonce.into(),
                    gas_price: value.execution_info.gas_price,
                    gas_limit: value.execution_info.gas_limit.into(),
                    to: TxKind::from(value.execution_info.to.map(Into::into)),
                    value: value.execution_info.value.into(),
                    input: value.execution_info.input.clone().into(),
                },
                signature,
                value.transaction_info.hash.into(),
            )),
        };

        Self {
            inner: Recovered::new_unchecked(inner, signer.into()),
            block_hash: None,
            block_number: None,
            transaction_index: None,
            effective_gas_price: Some(value.execution_info.gas_price),
        }
    }
}
