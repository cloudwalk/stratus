use ethers_core::types::Transaction as EthersTransaction;
use rlp::Decodable;

use crate::derive_newtype_from;
use crate::eth::primitives::Address;
use crate::eth::primitives::Hash;
use crate::eth::EthError;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Transaction(EthersTransaction);

impl Transaction {
    /// Transaction signer. Can be different from `from`, specially if `from` was not specified.
    pub fn signer(&self) -> Result<Address, EthError> {
        match self.0.recover_from() {
            Ok(signer) => Ok(signer.into()),
            Err(e) => {
                tracing::warn!(reason = ?e, "failed to recover transaction signer");
                Err(EthError::UnrecoverableSigner)
            }
        }
    }

    /// Field `from`. Defaults to zero if not specified.
    pub fn from(&self) -> Address {
        self.0.from.into()
    }

    /// Field `to`.
    pub fn to(&self) -> Option<Address> {
        self.0.to.map(|x| x.into())
    }

    /// Field `input`.
    pub fn input(&self) -> Vec<u8> {
        self.0.input.to_vec()
    }

    /// Computed transaction hash.
    pub fn hash(&self) -> Hash {
        self.0.hash().into()
    }
}

impl Decodable for Transaction {
    fn decode(rlp: &rlp::Rlp) -> Result<Self, rlp::DecoderError> {
        let inner = EthersTransaction::decode(rlp)?;
        Ok(Self(inner))
    }
}

// -----------------------------------------------------------------------------
// Other -> Self
// -----------------------------------------------------------------------------
derive_newtype_from!(self = Transaction, other = EthersTransaction);
