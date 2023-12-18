use std::fmt::Display;
use std::str::FromStr;

use ethereum_types::U64;

use crate::derive_newtype_from;
use crate::eth::EthError;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, derive_more::Add, derive_more::Sub)]
#[serde(transparent)]
pub struct BlockNumber(U64);

impl BlockNumber {
    pub const ZERO: BlockNumber = BlockNumber(U64::zero());

    /// Checks if the block number is the genesis block.
    pub fn is_genesis(&self) -> bool {
        self.0.as_usize() == 0
    }
}

impl Display for BlockNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
derive_newtype_from!(self = BlockNumber, other = U64, u8, u16, u32, u64, usize);

impl FromStr for BlockNumber {
    type Err = EthError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match U64::from_str(s) {
            Ok(parsed) => Ok(Self(parsed)),
            Err(e) => {
                tracing::warn!(reason = ?e, value = %s, "failed to parse block number selection");
                Err(EthError::parsing("blockNumber", s.to_owned()))
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<BlockNumber> for U64 {
    fn from(block_number: BlockNumber) -> Self {
        block_number.0
    }
}
