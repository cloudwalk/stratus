use std::fmt::Display;

use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub enum BlockFilter {
    /// Retrieve the most recent block.
    #[default]
    Latest,

    /// Retrieve the most early block.
    Earliest,

    /// Retrieve a block by its hash.
    Hash(Hash),

    /// Retrieve a block by its number.
    Number(BlockNumber),
}

impl Display for BlockFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockFilter::Latest => write!(f, "latest"),
            BlockFilter::Earliest => write!(f, "earliest"),
            BlockFilter::Hash(block_hash) => write!(f, "{}", block_hash),
            BlockFilter::Number(block_number) => write!(f, "{}", block_number),
        }
    }
}

// -----------------------------------------------------------------------------
// Serialization / Deserilization
// -----------------------------------------------------------------------------

impl<'de> serde::Deserialize<'de> for BlockFilter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?.to_lowercase();
        match value.as_str() {
            // parse special keywords
            "latest" => Ok(Self::Latest),
            "pending" => Ok(Self::Latest), // map to latest because we do not have the concept of pending blocks internally
            "earliest" => Ok(Self::Earliest),

            // parse hash (64: H256 without 0x prefix; 66: H256 with 0x prefix)
            s if s.len() == 64 || s.len() == 66 => {
                let hash: Hash = s.parse().map_err(serde::de::Error::custom)?;
                Ok(Self::Hash(hash))
            }
            // parse number
            s => {
                let number: BlockNumber = s.parse().map_err(serde::de::Error::custom)?;
                Ok(Self::Number(number))
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::eth::primitives::*;

    #[test]
    fn serde_block_number_with_latest() {
        let json = json!("latest");
        assert_eq!(serde_json::from_value::<BlockFilter>(json).unwrap(), BlockFilter::Latest);
    }

    #[test]
    fn serde_block_number_with_number() {
        let json = json!("0x2");
        assert_eq!(serde_json::from_value::<BlockFilter>(json).unwrap(), BlockFilter::Number(2usize.into()));
    }
}
