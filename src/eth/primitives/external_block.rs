use serde::Deserialize;

use crate::alias::AlloyBlockAlloyExternalTransaction;
use crate::alias::JsonValue;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::UnixTime;
use crate::log_and_err;

#[derive(Debug, Clone, derive_more::Deref, derive_more::DerefMut, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalBlock(#[deref] pub AlloyBlockAlloyExternalTransaction);

impl ExternalBlock {
    /// Returns the block hash.
    #[allow(clippy::expect_used)]
    pub fn hash(&self) -> Hash {
        Hash::from(self.0.header.hash)
    }

    /// Returns the block number.
    #[allow(clippy::expect_used)]
    pub fn number(&self) -> BlockNumber {
        BlockNumber::from(self.0.header.inner.number)
    }

    /// Returns the block timestamp.
    pub fn timestamp(&self) -> UnixTime {
        self.0.header.inner.timestamp.into()
    }

    /// Returns the block author.
    pub fn author(&self) -> Address {
        self.0.header.inner.beneficiary.into() // TODO: improve before merging - author -> beneficiary?
    }
}

// -----------------------------------------------------------------------------
// Serialization / Deserialization
// -----------------------------------------------------------------------------

// TODO: validate if we need to implement custom deserializer

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl TryFrom<JsonValue> for ExternalBlock {
    type Error = anyhow::Error;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        match ExternalBlock::deserialize(&value) {
            Ok(v) => Ok(v),
            Err(e) => log_and_err!(reason = e, payload = value, "failed to convert payload value to ExternalBlock"),
        }
    }
}
