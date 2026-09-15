use alloy_consensus::Signed;
use alloy_consensus::Transaction;
use alloy_consensus::TxEip1559;
use alloy_consensus::TxEip2930;
use alloy_consensus::TxEip4844;
use alloy_consensus::TxEip4844Variant;
use alloy_consensus::TxEip7702;
use alloy_consensus::TxEnvelope;
use alloy_consensus::TxLegacy;
use alloy_consensus::TxType;
use alloy_consensus::transaction::Recovered;
use alloy_eips::eip7702::SignedAuthorization;
use alloy_primitives::B256;
use alloy_primitives::Signature as AlloySignature;
use alloy_primitives::TxKind;
use alloy_primitives::U64;
use alloy_primitives::U256;
use alloy_primitives::keccak256;
use alloy_rlp::Decodable as RlpDecodable;
use alloy_rlp::Encodable as RlpEncodable;
use alloy_rlp::Header as RlpHeader;
use alloy_rlp::length_of_length;
use alloy_rpc_types_eth::AccessList;
use anyhow::Context;
use display_json::DebugAsJson;

use crate::alias::AlloyTransaction;
use crate::eth::executor::TransactionExecutionInput;
use crate::eth::rpc::TransactionDecodeError;
use crate::eth::types::Address;
use crate::eth::types::Bytes;
use crate::eth::types::ChainId;
use crate::eth::types::ExternalTransaction;
use crate::eth::types::Gas;
use crate::eth::types::Hash;
use crate::eth::types::Nonce;
use crate::eth::types::SignatureComponent;
use crate::eth::types::Wei;
use crate::ext::RuintExt;

/// Legacy transaction `v` value constants (EIP-155).
const LEGACY_V_UNPROTECTED_EVEN: u64 = 27;
const LEGACY_V_UNPROTECTED_ODD: u64 = 28;
const EIP155_V_OFFSET: u64 = 35;

/// Decodes the next RLP value, mapping a missing value to `MissingField` and an RLP error to `RlpError`.
fn decode_next<T: RlpDecodable>(rlp: &mut alloy_rlp::Rlp<'_>, field: &'static str) -> Result<T, TransactionDecodeError> {
    rlp.get_next()
        .map_err(|e| TransactionDecodeError::RlpError(e.to_string()))?
        .ok_or(TransactionDecodeError::MissingField(field))
}

/// Decodes the `to` field: empty bytes mean contract creation, otherwise a 20-byte address.
fn decode_to_field(rlp: &mut alloy_rlp::Rlp<'_>) -> Result<Option<Address>, TransactionDecodeError> {
    let to_bytes = decode_next::<Bytes>(rlp, "to")?;
    if to_bytes.is_empty() {
        Ok(None)
    } else {
        Ok(Some(Address::try_from(to_bytes.as_ref())?))
    }
}

/// Common fields shared by EIP-2930, EIP-1559, EIP-4844, and EIP-7702 transactions.
struct TypedTxCommonFields {
    gas_limit: Gas,
    to: Option<Address>,
    value: Wei,
    input: Bytes,
}

#[derive(DebugAsJson, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(serde::Deserialize, fake::Dummy))]
pub struct TransactionInfo {
    #[cfg_attr(test, dummy(expr = "crate::utils::test_utils::fake_option_uint()"))]
    pub tx_type: Option<U64>,
    pub hash: Hash,
}

#[derive(Clone, PartialEq, Eq, serde::Serialize, Default)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub enum Signer {
    Recovered(Address),
    #[default]
    Unrecovered,
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

#[derive(DebugAsJson, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(serde::Deserialize, fake::Dummy))]
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

#[derive(DebugAsJson, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(serde::Deserialize, fake::Dummy))]
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

#[derive(DebugAsJson, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(serde::Deserialize, fake::Dummy))]
pub struct TransactionInput {
    pub transaction_info: TransactionInfo,
    pub execution_info: ExecutionInfo,
    pub signature: Signature,
}

impl TransactionInput {
    /// Returns the recovered signer address.
    ///
    /// If the signer has not been recovered yet, this method recovers it on demand
    /// from the transaction fields and logs a warning so that missing recovery calls
    /// are visible.
    pub fn signer(&self) -> Address {
        if let Some(addr) = self.execution_info.signer.address() {
            return addr;
        }

        tracing::warn!(tx_hash = %self.transaction_info.hash, "Transaction signer was not recovered before accessing it; recovering on demand");
        match self.recover_signer_address() {
            Ok(addr) => addr,
            Err(e) => {
                tracing::error!(tx_hash = %self.transaction_info.hash, error = ?e, "failed to recover transaction signer on demand");
                Address::ZERO
            }
        }
    }

    /// Encodes a list of RLP items and returns the encoded bytes.
    fn encode_rlp_list(items: &[&dyn RlpEncodable]) -> Vec<u8> {
        let payload_length: usize = items.iter().map(|item| item.length()).sum();
        let mut out = Vec::with_capacity(payload_length + length_of_length(payload_length));
        RlpHeader { list: true, payload_length }.encode(&mut out);
        for item in items {
            item.encode(&mut out);
        }
        out
    }

    /// Encodes a transaction for signature hash computation.
    ///
    /// For typed transactions, the type byte is prepended to the RLP list.
    /// For legacy transactions, only the RLP list is encoded.
    fn encode_transaction(tx_type: Option<TxType>, fields: &[&dyn RlpEncodable]) -> B256 {
        let encoded = Self::encode_rlp_list(fields);
        let hash_input = match tx_type {
            Some(tx_type) => {
                let mut out = Vec::with_capacity(1 + encoded.len());
                out.push(tx_type as u8);
                out.extend_from_slice(&encoded);
                out
            }
            None => encoded,
        };
        B256::from(keccak256(hash_input))
    }

    /// Returns the RLP encoding of the `to` field: empty bytes for contract creation,
    /// or the 20-byte address for a call.
    fn encode_to(&self) -> Vec<u8> {
        self.execution_info.to.map(|addr| addr.0.to_vec()).unwrap_or_default()
    }

    /// Returns the RLP encoding of the transaction `input` data.
    fn encode_input(&self) -> Vec<u8> {
        self.execution_info.input.0.to_vec()
    }

    /// Computes the transaction signature hash from the fields stored in this input.
    ///
    /// Encodes the unsigned transaction directly via RLP.
    fn signature_hash(&self) -> B256 {
        let chain_id = self.execution_info.chain_id.map(|c| c.0.as_u64()).unwrap_or_default();
        let nonce = self.execution_info.nonce.as_u64();
        let gas_limit = self.execution_info.gas_limit.as_u64();
        let gas_price = self.execution_info.gas_price;
        let value = self.execution_info.value.0;
        let to = self.encode_to();
        let input = self.encode_input();

        let tx_type = self
            .transaction_info
            .tx_type
            .and_then(|t| TxType::try_from(t.as_u64()).ok())
            .unwrap_or(TxType::Legacy);

        match tx_type {
            TxType::Eip2930 => Self::encode_transaction(
                Some(TxType::Eip2930),
                &[
                    &chain_id,
                    &nonce,
                    &gas_price,
                    &gas_limit,
                    &to.as_slice(),
                    &value,
                    &input.as_slice(),
                    &AccessList::default(),
                ],
            ),

            TxType::Eip1559 => Self::encode_transaction(
                Some(TxType::Eip1559),
                &[
                    &chain_id,
                    &nonce,
                    &gas_price, // max_priority_fee_per_gas
                    &gas_price, // max_fee_per_gas
                    &gas_limit,
                    &to.as_slice(),
                    &value,
                    &input.as_slice(),
                    &AccessList::default(),
                ],
            ),

            TxType::Eip4844 => Self::encode_transaction(
                Some(TxType::Eip4844),
                &[
                    &chain_id,
                    &nonce,
                    &gas_price, // max_priority_fee_per_gas
                    &gas_price, // max_fee_per_gas
                    &gas_limit,
                    &to.as_slice(),
                    &value,
                    &input.as_slice(),
                    &AccessList::default(),
                    &0u128,              // max_fee_per_blob_gas
                    &Vec::<B256>::new(), // blob_versioned_hashes
                ],
            ),

            TxType::Eip7702 => Self::encode_transaction(
                Some(TxType::Eip7702),
                &[
                    &chain_id,
                    &nonce,
                    &gas_price, // max_priority_fee_per_gas
                    &gas_price, // max_fee_per_gas
                    &gas_limit,
                    &to.as_slice(),
                    &value,
                    &input.as_slice(),
                    &AccessList::default(),
                    &Vec::<SignedAuthorization>::new(), // authorization list placeholder
                ],
            ),

            TxType::Legacy =>
                if self.execution_info.chain_id.is_some() {
                    Self::encode_transaction(
                        None,
                        &[&nonce, &gas_price, &gas_limit, &to.as_slice(), &value, &input.as_slice(), &chain_id, &0u8, &0u8],
                    )
                } else {
                    Self::encode_transaction(None, &[&nonce, &gas_price, &gas_limit, &to.as_slice(), &value, &input.as_slice()])
                },
        }
    }

    /// Recovers the signer address from the transaction fields already stored in this input.
    fn recover_signer_address(&self) -> anyhow::Result<Address> {
        let prehash = self.signature_hash();
        let signature: AlloySignature = self.signature.into();
        let signer = signature
            .recover_address_from_prehash(&prehash)
            .context("Transaction signer cannot be recovered. Check the transaction signature is valid.")?;
        Ok(Address::from(signer))
    }

    /// Builds a TxEnvelope from the transaction fields stored in this input.
    fn to_tx_envelope(&self) -> TxEnvelope {
        let signature: AlloySignature = self.signature.into();
        let tx_hash = self.transaction_info.hash.into();

        let tx_type = self
            .transaction_info
            .tx_type
            .and_then(|t| TxType::try_from(t.as_u64()).ok())
            .unwrap_or(TxType::Legacy);

        match tx_type {
            TxType::Eip2930 => TxEnvelope::Eip2930(Signed::new_unchecked(
                TxEip2930 {
                    chain_id: self.execution_info.chain_id.unwrap_or_default().into(),
                    nonce: self.execution_info.nonce.into(),
                    gas_price: self.execution_info.gas_price,
                    gas_limit: self.execution_info.gas_limit.into(),
                    to: TxKind::from(self.execution_info.to.map(Into::into)),
                    value: self.execution_info.value.into(),
                    input: self.execution_info.input.clone().into(),
                    access_list: AccessList::default(),
                },
                signature,
                tx_hash,
            )),

            TxType::Eip1559 => TxEnvelope::Eip1559(Signed::new_unchecked(
                TxEip1559 {
                    chain_id: self.execution_info.chain_id.unwrap_or_default().into(),
                    nonce: self.execution_info.nonce.into(),
                    max_fee_per_gas: self.execution_info.gas_price,
                    max_priority_fee_per_gas: self.execution_info.gas_price,
                    gas_limit: self.execution_info.gas_limit.into(),
                    to: TxKind::from(self.execution_info.to.map(Into::into)),
                    value: self.execution_info.value.into(),
                    input: self.execution_info.input.clone().into(),
                    access_list: AccessList::default(),
                },
                signature,
                tx_hash,
            )),

            TxType::Eip4844 => TxEnvelope::Eip4844(Signed::new_unchecked(
                TxEip4844Variant::TxEip4844(TxEip4844 {
                    chain_id: self.execution_info.chain_id.unwrap_or_default().into(),
                    nonce: self.execution_info.nonce.into(),
                    max_fee_per_gas: self.execution_info.gas_price,
                    max_priority_fee_per_gas: self.execution_info.gas_price,
                    gas_limit: self.execution_info.gas_limit.into(),
                    to: self.execution_info.to.map(Into::into).unwrap_or_default(),
                    value: self.execution_info.value.into(),
                    input: self.execution_info.input.clone().into(),
                    access_list: AccessList::default(),
                    blob_versioned_hashes: Vec::default(),
                    max_fee_per_blob_gas: 0,
                }),
                signature,
                tx_hash,
            )),

            TxType::Eip7702 => TxEnvelope::Eip7702(Signed::new_unchecked(
                TxEip7702 {
                    chain_id: self.execution_info.chain_id.unwrap_or_default().into(),
                    nonce: self.execution_info.nonce.into(),
                    gas_limit: self.execution_info.gas_limit.into(),
                    max_fee_per_gas: self.execution_info.gas_price,
                    max_priority_fee_per_gas: self.execution_info.gas_price,
                    to: self.execution_info.to.map(Into::into).unwrap_or_default(),
                    value: self.execution_info.value.into(),
                    input: self.execution_info.input.clone().into(),
                    access_list: AccessList::default(),
                    authorization_list: Vec::default(),
                },
                signature,
                tx_hash,
            )),

            TxType::Legacy => TxEnvelope::Legacy(Signed::new_unchecked(
                TxLegacy {
                    chain_id: self.execution_info.chain_id.map(Into::into),
                    nonce: self.execution_info.nonce.into(),
                    gas_price: self.execution_info.gas_price,
                    gas_limit: self.execution_info.gas_limit.into(),
                    to: TxKind::from(self.execution_info.to.map(Into::into)),
                    value: self.execution_info.value.into(),
                    input: self.execution_info.input.clone().into(),
                },
                signature,
                tx_hash,
            )),
        }
    }
}

// -----------------------------------------------------------------------------
// Serialization / Deserialization
// -----------------------------------------------------------------------------

impl TransactionInput {
    /// Derives the chain id and signature parity from a legacy `v` value.
    fn decode_legacy_v(v: U64) -> Result<(Option<ChainId>, U64), TransactionDecodeError> {
        let v_raw = v.as_u64();
        match v_raw {
            LEGACY_V_UNPROTECTED_EVEN => Ok((None, U64::ZERO)),
            LEGACY_V_UNPROTECTED_ODD => Ok((None, U64::ONE)),
            v if v >= EIP155_V_OFFSET => {
                let chain_id = (v - EIP155_V_OFFSET) / 2;
                let parity = (v - LEGACY_V_UNPROTECTED_EVEN) % 2;
                Ok((Some(ChainId::from(chain_id)), U64::from(parity)))
            }
            _ => Err(TransactionDecodeError::InvalidLegacyV),
        }
    }

    /// Builds a `TransactionInput` from decoded legacy fields and recovers the signer.
    #[allow(clippy::too_many_arguments)]
    fn build_legacy(
        nonce: Nonce,
        gas_price: u128,
        gas_limit: Gas,
        to: Option<Address>,
        value: Wei,
        input: Bytes,
        v: U64,
        r: U256,
        s: U256,
        hash: Hash,
    ) -> Result<Self, TransactionDecodeError> {
        let (chain_id, parity) = Self::decode_legacy_v(v)?;

        let mut tx = Self {
            transaction_info: TransactionInfo { tx_type: None, hash },
            execution_info: ExecutionInfo {
                chain_id,
                nonce,
                signer: Signer::Unrecovered,
                to,
                value,
                input,
                gas_limit,
                gas_price,
            },
            signature: Signature { v: parity, r, s },
        };

        let signer = tx.recover_signer_address().map_err(|_| TransactionDecodeError::SignerRecovery)?;
        tx.execution_info.signer = Signer::Recovered(signer);

        Ok(tx)
    }

    /// Decodes a legacy transaction from raw RLP bytes.
    fn decode_legacy(raw_bytes: &[u8]) -> Result<Self, TransactionDecodeError> {
        let mut rlp = alloy_rlp::Rlp::new(raw_bytes).map_err(|e| TransactionDecodeError::RlpError(e.to_string()))?;

        let nonce = decode_next::<Nonce>(&mut rlp, "nonce")?;
        let gas_price = decode_next::<u128>(&mut rlp, "gasPrice")?;
        let gas_limit = decode_next::<Gas>(&mut rlp, "gasLimit")?;
        let to = decode_to_field(&mut rlp)?;
        let value = decode_next::<Wei>(&mut rlp, "value")?;
        let input = decode_next::<Bytes>(&mut rlp, "input")?;
        let v = decode_next::<U64>(&mut rlp, "v")?;
        let r = decode_next::<U256>(&mut rlp, "r")?;
        let s = decode_next::<U256>(&mut rlp, "s")?;

        if rlp.get_next::<u8>().map_err(|e| TransactionDecodeError::RlpError(e.to_string()))?.is_some() {
            return Err(TransactionDecodeError::ExtraFields);
        }

        let hash = Hash::from(keccak256(raw_bytes));

        Self::build_legacy(nonce, gas_price, gas_limit, to, value, input, v, r, s, hash)
    }

    /// Decodes the common fields shared by access-list transaction types (EIP-2930, EIP-1559, EIP-4844, and EIP-7702).
    fn decode_access_list_fields(rlp: &mut alloy_rlp::Rlp<'_>) -> Result<TypedTxCommonFields, TransactionDecodeError> {
        let gas_limit = decode_next::<Gas>(rlp, "gasLimit")?;
        let to = decode_to_field(rlp)?;
        let value = decode_next::<Wei>(rlp, "value")?;
        let input = decode_next::<Bytes>(rlp, "input")?;
        let _: AccessList = decode_next(rlp, "accessList")?;

        Ok(TypedTxCommonFields { gas_limit, to, value, input })
    }

    /// Decodes the dynamic-fee gas price fields (`maxPriorityFeePerGas` and `maxFeePerGas`)
    /// and returns `maxFeePerGas` as the effective gas price.
    fn decode_dynamic_fee_gas_price(rlp: &mut alloy_rlp::Rlp<'_>) -> Result<u128, TransactionDecodeError> {
        let max_priority_fee_per_gas = decode_next::<u128>(rlp, "maxPriorityFeePerGas")?;
        let max_fee_per_gas = decode_next::<u128>(rlp, "maxFeePerGas")?;
        let _ = max_priority_fee_per_gas;
        Ok(max_fee_per_gas)
    }

    /// Decodes the transaction signature fields (`v`, `r`, `s`).
    fn decode_signature(rlp: &mut alloy_rlp::Rlp<'_>) -> Result<(U64, U256, U256), TransactionDecodeError> {
        let v = decode_next::<U64>(rlp, "v")?;
        let r = decode_next::<U256>(rlp, "r")?;
        let s = decode_next::<U256>(rlp, "s")?;
        Ok((v, r, s))
    }

    /// Decodes a typed transaction (EIP-2718) from raw bytes.
    fn decode_typed(tx_type: u8, payload: &[u8], raw_bytes: &[u8]) -> Result<Self, TransactionDecodeError> {
        let tx_type = TxType::try_from(tx_type).map_err(|_| TransactionDecodeError::UnsupportedType)?;
        let mut rlp = alloy_rlp::Rlp::new(payload).map_err(|e| TransactionDecodeError::RlpError(e.to_string()))?;

        let chain_id = decode_next::<ChainId>(&mut rlp, "chainId")?;
        let nonce = decode_next::<Nonce>(&mut rlp, "nonce")?;
        let gas_price: u128;
        let gas_limit: Gas;
        let to: Option<Address>;
        let value: Wei;
        let input: Bytes;
        let v: U64;
        let r: U256;
        let s: U256;

        match tx_type {
            TxType::Eip2930 => {
                gas_price = decode_next::<u128>(&mut rlp, "gasPrice")?;
                let fields = Self::decode_access_list_fields(&mut rlp)?;
                gas_limit = fields.gas_limit;
                to = fields.to;
                value = fields.value;
                input = fields.input;
                (v, r, s) = Self::decode_signature(&mut rlp)?;
            }

            TxType::Eip1559 => {
                gas_price = Self::decode_dynamic_fee_gas_price(&mut rlp)?;
                let fields = Self::decode_access_list_fields(&mut rlp)?;
                gas_limit = fields.gas_limit;
                to = fields.to;
                value = fields.value;
                input = fields.input;
                (v, r, s) = Self::decode_signature(&mut rlp)?;
            }

            TxType::Eip4844 => {
                gas_price = Self::decode_dynamic_fee_gas_price(&mut rlp)?;
                let fields = Self::decode_access_list_fields(&mut rlp)?;
                gas_limit = fields.gas_limit;
                to = fields.to;
                value = fields.value;
                input = fields.input;
                let _: u128 = decode_next(&mut rlp, "maxFeePerBlobGas")?;
                let _: Vec<B256> = decode_next(&mut rlp, "blobVersionedHashes")?;
                (v, r, s) = Self::decode_signature(&mut rlp)?;
            }

            TxType::Eip7702 => {
                gas_price = Self::decode_dynamic_fee_gas_price(&mut rlp)?;
                let fields = Self::decode_access_list_fields(&mut rlp)?;
                gas_limit = fields.gas_limit;
                to = fields.to;
                value = fields.value;
                input = fields.input;
                let _: Vec<SignedAuthorization> = decode_next(&mut rlp, "authorizationList")?;
                (v, r, s) = Self::decode_signature(&mut rlp)?;
            }

            TxType::Legacy => return Err(TransactionDecodeError::LegacyNotTyped),
        }

        if rlp.get_next::<u8>().map_err(|e| TransactionDecodeError::RlpError(e.to_string()))?.is_some() {
            return Err(TransactionDecodeError::ExtraFields);
        }

        let hash = Hash::from(keccak256(raw_bytes));

        let mut tx = Self {
            transaction_info: TransactionInfo {
                tx_type: Some(U64::from(tx_type as u8)),
                hash,
            },
            execution_info: ExecutionInfo {
                chain_id: Some(chain_id),
                nonce,
                signer: Signer::Unrecovered,
                to,
                value,
                input,
                gas_limit,
                gas_price,
            },
            signature: Signature { v, r, s },
        };

        let signer = tx.recover_signer_address().map_err(|_| TransactionDecodeError::SignerRecovery)?;
        tx.execution_info.signer = Signer::Recovered(signer);

        Ok(tx)
    }
}

impl RlpDecodable for TransactionInput {
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        let raw_bytes = *buf;

        if raw_bytes.is_empty() {
            return Err(TransactionDecodeError::EmptyBytes.into());
        }

        let tx = match raw_bytes[0] {
            byte if byte >= 0xc0 => Self::decode_legacy(raw_bytes).map_err(alloy_rlp::Error::from)?,
            byte if byte <= 0x7f => {
                let tx_type = byte;
                let payload = &raw_bytes[1..];
                Self::decode_typed(tx_type, payload, raw_bytes).map_err(alloy_rlp::Error::from)?
            }
            _ => return Err(TransactionDecodeError::InvalidTypeByte.into()),
        };

        // A raw transaction occupies the entire buffer.
        *buf = &[];
        Ok(tx)
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

fn build_transaction_input_from_envelope(envelope: &TxEnvelope) -> anyhow::Result<TransactionInput> {
    // Get signature components from the envelope
    let signature = envelope.signature();
    let signature = Signature {
        r: signature.r(),
        s: signature.s(),
        v: if signature.v() { U64::ONE } else { U64::ZERO },
    };

    // Build the TransactionInput from the fields we currently support, leaving the
    // signer unrecovered. We intentionally ignore any signer that may
    // already be present in the source transaction so that the leader and the follower always derive the same address
    // from the same set of saved fields.
    let mut tx_input = TransactionInput {
        transaction_info: TransactionInfo {
            tx_type: Some(U64::from(envelope.tx_type() as u8)),
            hash: Hash::from(*envelope.tx_hash()),
        },
        execution_info: ExecutionInfo {
            chain_id: envelope.chain_id().map(Into::into),
            nonce: Nonce::from(envelope.nonce()),
            signer: Signer::Unrecovered,
            to: match envelope.kind() {
                TxKind::Call(addr) => Some(Address::from(addr)),
                TxKind::Create => None,
            },
            value: Wei::from(envelope.value()),
            input: Bytes::from(envelope.input().clone()),
            gas_limit: Gas::from(envelope.gas_limit()),
            gas_price: envelope.max_fee_per_gas(),
        },
        signature,
    };

    // Recover the signer directly from the saved fields.
    let recovered_signer = tx_input.recover_signer_address()?;
    tx_input.execution_info.signer = Signer::Recovered(recovered_signer);

    Ok(tx_input)
}

fn try_from_alloy_transaction(value: alloy_rpc_types_eth::Transaction) -> anyhow::Result<TransactionInput> {
    build_transaction_input_from_envelope(value.inner.inner())
}

impl From<TransactionExecutionInput> for ExecutionInfo {
    fn from(value: TransactionExecutionInput) -> Self {
        Self {
            chain_id: value.chain_id,
            nonce: value.nonce,
            signer: Signer::Recovered(value.from),
            to: value.to,
            value: value.value,
            input: value.data,
            gas_limit: value.gas_limit,
            gas_price: value.gas_price,
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<TransactionInput> for AlloyTransaction {
    fn from(value: TransactionInput) -> Self {
        let signer = value.signer();
        let inner = value.to_tx_envelope();

        Self {
            inner: Recovered::new_unchecked(inner, signer.into()),
            block_hash: None,
            block_number: None,
            block_timestamp: None,
            transaction_index: None,
            effective_gas_price: Some(value.execution_info.gas_price),
        }
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use alloy_consensus::SignableTransaction;
    use alloy_consensus::Signed;
    use alloy_consensus::TxEip1559;
    use alloy_consensus::TxEip2930;
    use alloy_consensus::TxEip4844;
    use alloy_consensus::TxEip7702;
    use alloy_consensus::TxLegacy;
    use alloy_eips::eip2718::Encodable2718;
    use alloy_primitives::Address as AlloyAddress;
    use alloy_primitives::Bytes as AlloyBytes;
    use alloy_primitives::Signature as AlloySignature;
    use alloy_primitives::TxKind;
    use alloy_primitives::U64;
    use alloy_primitives::U256;
    use alloy_primitives::keccak256;

    use super::*;

    fn dummy_transaction_input(tx_type: u64) -> TransactionInput {
        TransactionInput {
            transaction_info: TransactionInfo {
                tx_type: Some(U64::from(tx_type)),
                hash: Hash::default(),
            },
            execution_info: ExecutionInfo {
                chain_id: Some(ChainId::from(1u64)),
                nonce: Nonce::from(1u64),
                signer: Signer::Unrecovered,
                to: Some(Address::default()),
                value: Wei::from(100u64),
                input: Bytes::default(),
                gas_limit: Gas::from(21000u64),
                gas_price: 1_000_000_000,
            },
            signature: Signature {
                v: U64::ZERO,
                r: U256::from(1u64),
                s: U256::from(2u64),
            },
        }
    }

    #[test]
    fn signature_hash_matches_to_tx_envelope() {
        for tx_type in [0, 1, 2, 3, 4] {
            let tx_input = dummy_transaction_input(tx_type);
            let from_signature_hash = tx_input.signature_hash();
            let from_envelope = tx_input.to_tx_envelope().signature_hash();
            assert_eq!(from_signature_hash, from_envelope, "signature_hash mismatch for tx_type {tx_type}");
        }
    }

    /// Builds raw EIP-2718 bytes for a transaction and decodes them directly into a
    /// `TransactionInput`, comparing the recovered fields and signer with the source transaction.
    fn assert_direct_decode<T>(tx: T, tx_type: u8)
    where
        T: alloy_consensus::SignableTransaction<AlloySignature> + alloy_consensus::transaction::RlpEcdsaEncodableTx + alloy_eips::Typed2718,
    {
        let signature = AlloySignature::test_signature();
        let signing_hash = tx.signature_hash();
        let expected_signer = signature
            .recover_address_from_prehash(&signing_hash)
            .expect("test signature should be recoverable");

        let signed = Signed::new_unchecked(tx, signature, B256::default());

        let mut raw_bytes = Vec::new();
        signed.encode_2718(&mut raw_bytes);
        let expected_hash = Hash::from(keccak256(&raw_bytes));

        let decoded = TransactionInput::decode(&mut raw_bytes.as_slice()).expect("direct RLP decode should succeed");

        assert_eq!(decoded.transaction_info.tx_type, Some(U64::from(tx_type)));
        assert_eq!(decoded.transaction_info.hash, expected_hash);
        assert_eq!(decoded.execution_info.chain_id, Some(ChainId::from(1u64)));
        assert_eq!(decoded.execution_info.nonce, Nonce::from(1u64));
        assert_eq!(decoded.execution_info.gas_price, 1_000_000_000);
        assert_eq!(decoded.execution_info.gas_limit, Gas::from(21000u64));
        assert_eq!(decoded.execution_info.to, Some(Address::default()));
        assert_eq!(decoded.execution_info.value, Wei::from(100u64));
        assert_eq!(decoded.execution_info.input, Bytes::default());
        assert_eq!(decoded.signer(), Address::from(expected_signer));
    }

    #[test]
    fn decode_legacy_from_raw_bytes() {
        let tx = TxLegacy {
            chain_id: Some(1),
            nonce: 1,
            gas_price: 1_000_000_000,
            gas_limit: 21000,
            to: TxKind::Call(AlloyAddress::default()),
            value: U256::from(100),
            input: AlloyBytes::new(),
        };

        let signature = AlloySignature::test_signature();
        let signing_hash = tx.signature_hash();
        let expected_signer = signature
            .recover_address_from_prehash(&signing_hash)
            .expect("test signature should be recoverable");

        let signed = Signed::new_unchecked(tx, signature, B256::default());

        let mut raw_bytes = Vec::new();
        signed.encode_2718(&mut raw_bytes);
        let expected_hash = Hash::from(keccak256(&raw_bytes));

        let decoded = TransactionInput::decode(&mut raw_bytes.as_slice()).expect("direct RLP decode should succeed");

        assert_eq!(decoded.transaction_info.tx_type, None);
        assert_eq!(decoded.transaction_info.hash, expected_hash);
        assert_eq!(decoded.execution_info.chain_id, Some(ChainId::from(1u64)));
        assert_eq!(decoded.execution_info.nonce, Nonce::from(1u64));
        assert_eq!(decoded.execution_info.gas_price, 1_000_000_000);
        assert_eq!(decoded.execution_info.gas_limit, Gas::from(21000u64));
        assert_eq!(decoded.execution_info.to, Some(Address::default()));
        assert_eq!(decoded.execution_info.value, Wei::from(100u64));
        assert_eq!(decoded.execution_info.input, Bytes::default());
        assert_eq!(decoded.signer(), Address::from(expected_signer));
    }

    #[test]
    fn decode_eip2930_from_raw_bytes() {
        let tx = TxEip2930 {
            chain_id: 1,
            nonce: 1,
            gas_price: 1_000_000_000,
            gas_limit: 21000,
            to: TxKind::Call(AlloyAddress::default()),
            value: U256::from(100),
            input: AlloyBytes::new(),
            access_list: AccessList::default(),
        };
        assert_direct_decode(tx, 1);
    }

    #[test]
    fn decode_eip1559_from_raw_bytes() {
        let tx = TxEip1559 {
            chain_id: 1,
            nonce: 1,
            max_priority_fee_per_gas: 1_000_000_000,
            max_fee_per_gas: 1_000_000_000,
            gas_limit: 21000,
            to: TxKind::Call(AlloyAddress::default()),
            value: U256::from(100),
            input: AlloyBytes::new(),
            access_list: AccessList::default(),
        };
        assert_direct_decode(tx, 2);
    }

    #[test]
    fn decode_eip4844_from_raw_bytes() {
        let tx = TxEip4844 {
            chain_id: 1,
            nonce: 1,
            max_priority_fee_per_gas: 1_000_000_000,
            max_fee_per_gas: 1_000_000_000,
            gas_limit: 21000,
            to: AlloyAddress::default(),
            value: U256::from(100),
            input: AlloyBytes::new(),
            access_list: AccessList::default(),
            blob_versioned_hashes: Vec::new(),
            max_fee_per_blob_gas: 0,
        };
        assert_direct_decode(tx, 3);
    }

    #[test]
    fn decode_eip7702_from_raw_bytes() {
        let tx = TxEip7702 {
            chain_id: 1,
            nonce: 1,
            max_priority_fee_per_gas: 1_000_000_000,
            max_fee_per_gas: 1_000_000_000,
            gas_limit: 21000,
            to: AlloyAddress::default(),
            value: U256::from(100),
            input: AlloyBytes::new(),
            access_list: AccessList::default(),
            authorization_list: Vec::new(),
        };
        assert_direct_decode(tx, 4);
    }
}
