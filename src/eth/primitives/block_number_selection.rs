use crate::eth::primitives::BlockNumber;

#[derive(Debug, PartialEq, Eq)]
pub enum BlockNumberSelection {
    Latest,
    Block(BlockNumber),
}

impl<'de> serde::Deserialize<'de> for BlockNumberSelection {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?.to_lowercase();
        match value.as_str() {
            "latest" => Ok(Self::Latest),
            s => {
                let number: BlockNumber = s.parse().map_err(serde::de::Error::custom)?;
                Ok(Self::Block(number))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn deserialize_block_number_with_latest() {
        let json = json!("latest");
        assert_eq!(serde_json::from_value::<BlockNumberSelection>(json).unwrap(), BlockNumberSelection::Latest);
    }

    #[test]
    fn deserialize_block_number_with_number() {
        let json = json!("0x2");
        assert_eq!(
            serde_json::from_value::<BlockNumberSelection>(json).unwrap(),
            BlockNumberSelection::Block(2usize.into())
        );
    }
}
