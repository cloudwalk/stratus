use std::borrow::Cow;

use crate::alias::EthersTransaction;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Signature;
use crate::eth::primitives::SoliditySignature;
use crate::ext::not;

#[derive(Debug, Clone, Default, derive_more:: Deref, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalTransaction(#[deref] pub EthersTransaction);

impl ExternalTransaction {
    /// Returns the block number where the transaction was mined.
    pub fn block_number(&self) -> BlockNumber {
        self.0.block_number.unwrap().into()
    }

    /// Returns the transaction hash.
    pub fn hash(&self) -> Hash {
        self.0.hash.into()
    }

    /// Checks if the current transaction is a contract deployment.
    pub fn is_contract_deployment(&self) -> bool {
        self.to.is_none() && not(self.input.is_empty())
    }

    /// Parses the Solidity function being called.
    ///
    /// TODO: unify and remove duplicate implementations.
    pub fn solidity_signature(&self) -> Option<SoliditySignature> {
        if self.is_contract_deployment() {
            return Some(Cow::from("contract_deployment"));
        }
        let sig = Signature::Function(self.input.get(..4)?.try_into().ok()?);
        Some(sig.extract())
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl From<EthersTransaction> for ExternalTransaction {
    fn from(value: EthersTransaction) -> Self {
        ExternalTransaction(value)
    }
}
