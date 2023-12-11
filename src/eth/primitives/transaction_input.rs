use ethers_core::types::Transaction as EthersTransaction;
use rlp::Decodable;

use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Hash;
use crate::eth::EthError;

#[derive(Debug, Clone, Default)]
pub struct TransactionInput {
    pub hash: Hash,
    pub from: Address,
    pub to: Option<Address>,
    pub input: Bytes,
    pub gas_limit: Gas,
    pub(super) inner: EthersTransaction,
}

impl TransactionInput {
    /// Transaction signer. Can be different from `from`, specially if `from` was not specified.
    pub fn signer(&self) -> Result<Address, EthError> {
        match self.inner.recover_from() {
            Ok(signer) => Ok(signer.into()),
            Err(e) => {
                tracing::warn!(reason = ?e, "failed to recover transaction signer");
                Err(EthError::UnrecoverableSigner)
            }
        }
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
            from: value.from.into(),
            to: value.to.map(|x| x.into()),
            input: value.input.clone().into(),
            gas_limit: value.gas.into(),
            inner: value,
        }
    }
}
