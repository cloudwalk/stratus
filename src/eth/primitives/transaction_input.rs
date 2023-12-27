use ethers_core::types::Transaction as EthersTransaction;
use rlp::Decodable;

use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Nonce;
use crate::eth::EthError;
use crate::ext::not;
use crate::ext::OptionExt;

#[derive(Debug, Clone, Default)]
pub struct TransactionInput {
    pub hash: Hash,
    pub nonce: Nonce,
    pub from: Address,
    pub to: Option<Address>,
    pub input: Bytes,
    pub gas: Gas,

    // TODO: Remove it from here. Use the same approach used in Block where operation are internally are delegated to the Ethers library
    // without having to keep the representation as part of the struct.
    pub(super) inner: EthersTransaction,
}

impl TransactionInput {
    /// Transaction signer (derived from the tranasaction signature).
    pub fn signer(&self) -> Result<Address, EthError> {
        match self.inner.recover_from() {
            Ok(signer) => Ok(signer.into()),
            Err(e) => {
                tracing::warn!(reason = ?e, "failed to recover transaction signer");
                Err(EthError::UnrecoverableSigner)
            }
        }
    }

    /// Checks if the current transaction is for a contract deployment.
    pub fn is_contract_deployment(&self) -> bool {
        self.to.is_none() && not(self.input.is_empty())
    }
}

// -----------------------------------------------------------------------------
// Serialization / Deserialization
// -----------------------------------------------------------------------------
impl serde::Serialize for TransactionInput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.inner.serialize(serializer)
    }
}

impl Decodable for TransactionInput {
    fn decode(rlp: &rlp::Rlp) -> Result<Self, rlp::DecoderError> {
        let inner = EthersTransaction::decode(rlp)?;
        Ok(Self::from(inner))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl From<EthersTransaction> for TransactionInput {
    fn from(value: EthersTransaction) -> Self {
        Self {
            hash: value.hash.into(),
            nonce: value.nonce.into(),
            from: value.from.into(),
            to: value.to.map_into(),
            input: value.input.clone().into(),
            gas: value.gas.into(),
            inner: value,
        }
    }
}
