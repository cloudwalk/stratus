use ethers_core::types::Block as EthersBlock;
use ethers_core::types::Transaction as EthersTransaction;

use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Gas;
use crate::eth::primitives::Hash;

#[derive(Debug, Clone, Default)]
pub struct Block {
    pub hash: Hash,
    pub number: BlockNumber,
    pub gas_used: Gas,

    pub(super) inner: EthersBlock<EthersTransaction>,
}

// -----------------------------------------------------------------------------
// Serialization / Deserialization
// -----------------------------------------------------------------------------
impl serde::Serialize for Block {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.inner.serialize(serializer)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl From<EthersBlock<EthersTransaction>> for Block {
    fn from(value: EthersBlock<EthersTransaction>) -> Self {
        Self {
            hash: value.hash.unwrap().into(),
            number: value.number.unwrap().into(),
            gas_used: value.gas_used.into(),
            inner: value,
        }
    }
}
