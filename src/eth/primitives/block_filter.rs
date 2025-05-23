use std::fmt::Display;

use display_json::DebugAsJson;

use super::PointInTime;
use crate::alias::JsonValue;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;

#[derive(DebugAsJson, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, Hash)]
#[cfg_attr(test, derive(fake::Dummy))]
pub enum BlockFilter {
    /// Information from the last mined block.
    #[default]
    Latest,

    /// Information from the block being mined.
    Pending,

    /// Information from the first block.
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
            BlockFilter::Pending => write!(f, "pending"),
            BlockFilter::Earliest => write!(f, "earliest"),
            BlockFilter::Hash(block_hash) => write!(f, "{block_hash}"),
            BlockFilter::Number(block_number) => write!(f, "{block_number}"),
        }
    }
}

impl From<PointInTime> for BlockFilter {
    fn from(point_in_time: PointInTime) -> Self {
        match point_in_time {
            PointInTime::Mined => Self::Latest,
            PointInTime::Pending => Self::Pending,
            PointInTime::MinedPast(number) => Self::Number(number),
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
        let value = JsonValue::deserialize(deserializer)?;
        match value {
            // default
            JsonValue::Null => Ok(Self::Latest),

            // number
            serde_json::Value::Number(number) => match number.as_u64() {
                Some(number) => Ok(Self::Number(BlockNumber::from(number))),
                None => Err(serde::de::Error::custom("block filter must be zero or a positive integer")),
            },

            // string
            serde_json::Value::String(value) => {
                match value.as_str() {
                    // parse special keywords
                    "latest" | "Latest" => Ok(Self::Latest),
                    "pending" | "Pending" => Ok(Self::Pending),
                    "earliest" | "Earliest" => Ok(Self::Earliest),

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

            serde_json::Value::Object(map) => {
                if map.len() != 1 {
                    return Err(serde::de::Error::custom("value was an object with an unexpected number of fields"));
                }
                let Some((key, value)) = map.iter().next() else {
                    return Err(serde::de::Error::custom("value was an object with no fields"));
                };
                let Some(value_str) = value.as_str() else {
                    return Err(serde::de::Error::custom("value was an object with non-str fields"));
                };
                match key.as_str() {
                    "Hash" => {
                        let hash: Hash = value_str.parse().map_err(serde::de::Error::custom)?;
                        Ok(Self::Hash(hash))
                    }
                    "Number" => {
                        let number: BlockNumber = value_str.parse().map_err(serde::de::Error::custom)?;
                        Ok(Self::Number(number))
                    }
                    _ => Err(serde::de::Error::custom(
                        "value was an object but its field was neither \"Hash\" nor \"Number\"",
                    )),
                }
            }

            // unhandled type
            _ => Err(serde::de::Error::custom("block filter must be a string or integer")),
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
