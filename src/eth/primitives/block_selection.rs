//! Block Selection Module
//!
//! This module offers mechanisms for selecting specific blocks from the
//! Ethereum blockchain. It allows for the retrieval of blocks based on
//! different criteria, such as the most recent block (`Latest`), the earliest
//! block in the chain (`Earliest`), a block by its hash (`Hash`), or a block
//! by its number (`Number`). This flexibility is crucial in various blockchain
//! operations, including querying block information and validating chain
//! integrity.

use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockSelection {
    /// Retrieve the most recent block.
    Latest,

    /// Retrieve the most early block.
    Earliest,

    /// Retrieve a block by its hash.
    Hash(Hash),

    /// Retrieve a block by its number.
    Number(BlockNumber),
}

impl Default for BlockSelection {
    fn default() -> Self {
        Self::Latest
    }
}

impl<'de> serde::Deserialize<'de> for BlockSelection {
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::eth::primitives::*;

    #[test]
    fn serde_block_number_with_latest() {
        let json = json!("latest");
        assert_eq!(serde_json::from_value::<BlockSelection>(json).unwrap(), BlockSelection::Latest);
    }

    #[test]
    fn serde_block_number_with_number() {
        let json = json!("0x2");
        assert_eq!(serde_json::from_value::<BlockSelection>(json).unwrap(), BlockSelection::Number(2usize.into()));
    }
}
