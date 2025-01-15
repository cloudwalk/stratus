use serde::Deserialize;

use crate::alias::EthersBlockExternalTransaction;
use crate::alias::JsonValue;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::UnixTime;
use crate::log_and_err;

#[derive(Debug, Clone, derive_more::Deref, derive_more::DerefMut, serde::Deserialize, serde::Serialize)]
#[serde(transparent)]
pub struct ExternalBlock(#[deref] pub EthersBlockExternalTransaction);

impl ExternalBlock {
    /// Returns the block hash.
    #[allow(clippy::expect_used)]
    pub fn hash(&self) -> Hash {
        self.0.hash.expect("external block must have hash").into()
    }

    /// Returns the block number.
    #[allow(clippy::expect_used)]
    pub fn number(&self) -> BlockNumber {
        self.0.number.expect("external block must have number").into()
    }

    /// Returns the block timestamp.
    pub fn timestamp(&self) -> UnixTime {
        self.0.timestamp.into()
    }

    /// Returns the block timestamp.
    pub fn author(&self) -> Address {
        self.0.author.unwrap_or_default().into()
    }
}

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
