use std::borrow::Cow;

use anyhow::anyhow;
use display_json::DebugAsJson;
use ethereum_types::U256;
use ethereum_types::U64;
use ethers_core::types::NameOrAddress;
use ethers_core::types::Transaction as EthersTransaction;
use ethers_core::types::TransactionRequest;
use fake::Dummy;
use fake::Fake;
use fake::Faker;
use rlp::Decodable;

use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ChainId;
use crate::eth::primitives::ExternalTransaction;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Nonce;
use crate::eth::primitives::Signature;
use crate::eth::primitives::SoliditySignature;
use crate::eth::primitives::Wei;
use crate::ext::not;
use crate::ext::OptionExt;
use crate::log_and_err;

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

impl TransactionInput {
    /// Checks if the current transaction is for a contract deployment.
    pub fn is_contract_deployment(&self) -> bool {
        self.to.is_none() && not(self.input.is_empty())
    }

    pub fn extract_function(&self) -> Option<SoliditySignature> {
        if self.is_contract_deployment() {
            return Some(Cow::from("contract_deployment"));
        }
        let sig = Signature::Function(self.input.get(..4)?.try_into().ok()?);

        Some(sig.extract())
    }
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
            to: faker.fake_with_rng(rng),
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
        let ethers_transaction = EthersTransaction::decode(rlp)?;
        match Self::try_from(ethers_transaction) {
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
        try_from_ethers_transaction(value.0, false)
    }
}

impl TryFrom<EthersTransaction> for TransactionInput {
    type Error = anyhow::Error;

    fn try_from(value: EthersTransaction) -> anyhow::Result<Self> {
        try_from_ethers_transaction(value, true)
    }
}

fn try_from_ethers_transaction(value: EthersTransaction, compute_signer: bool) -> anyhow::Result<TransactionInput> {
    // extract signer
    let signer: Address = match compute_signer {
        true => match value.recover_from() {
            Ok(signer) => signer.into(),
            Err(e) => {
                tracing::warn!(reason = ?e, "failed to recover transaction signer");
                return Err(anyhow!("Transaction signer cannot be recovered. Check the transaction signature is valid."));
            }
        },
        false => value.from.into(),
    };

    // extract gas price
    let gas_price: Wei = match value.gas_price {
        Some(wei) => wei.into(),
        None => return log_and_err!("transaction without gas_price id is not allowed"),
    };

    Ok(TransactionInput {
        tx_type: value.transaction_type,
        chain_id: match value.chain_id {
            Some(chain_id) => Some(chain_id.try_into()?),
            None => None,
        },
        hash: value.hash.into(),
        nonce: value.nonce.try_into()?,
        signer,
        from: Address::new(value.from.into()),
        to: value.to.map_into(),
        value: value.value.into(),
        input: value.input.clone().into(),
        gas_limit: value.gas.try_into()?,
        gas_price,
        v: value.v,
        r: value.r,
        s: value.s,
    })
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<TransactionInput> for EthersTransaction {
    fn from(value: TransactionInput) -> Self {
        Self {
            chain_id: value.chain_id.map_into(),
            hash: value.hash.into(),
            nonce: value.nonce.into(),
            from: value.signer.into(),
            to: value.to.map_into(),
            value: value.value.into(),
            input: value.input.clone().into(),
            gas: value.gas_limit.into(),
            gas_price: Some(value.gas_price.into()),
            v: value.v,
            r: value.r,
            s: value.s,
            transaction_type: value.tx_type,
            ..Default::default()
        }
    }
}

impl From<TransactionInput> for TransactionRequest {
    fn from(value: TransactionInput) -> Self {
        let input = value;
        Self {
            chain_id: input.chain_id.map(|id| id.inner_value()),
            nonce: Some(input.nonce.into()),
            from: Some(input.signer.into()),
            to: input.to.map(|to| NameOrAddress::Address(to.into())),
            value: Some(input.value.into()),
            gas_price: Some(input.gas_price.into()),
            gas: Some(input.gas_limit.into()),
            data: Some(input.input.into()),
        }
    }
}
