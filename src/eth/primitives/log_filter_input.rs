use std::ops::Deref;
use std::sync::Arc;

use display_json::DebugAsJson;
use serde_with::DefaultOnNull;
use serde_with::OneOrMany;
use serde_with::PickFirst;
use serde_with::formats::PreferMany;
use serde_with::serde_as;

use crate::eth::primitives::Address;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogTopic;
use crate::eth::primitives::PointInTime;
use crate::eth::primitives::RpcError;
use crate::eth::primitives::StratusError;
use crate::eth::storage::StratusStorage;

/// JSON-RPC input used in methods like `eth_getLogs` and `eth_subscribe`.
#[serde_as]
#[derive(DebugAsJson, Clone, Default, serde::Deserialize, serde::Serialize, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct LogFilterInput {
    #[serde(rename = "fromBlock", default)]
    pub from_block: Option<BlockFilter>,

    #[serde(rename = "toBlock", default)]
    pub to_block: Option<BlockFilter>,

    #[serde(rename = "blockHash", default)]
    pub block_hash: Option<Hash>,

    #[serde(rename = "address", default)]
    #[serde_as(deserialize_as = "PickFirst<(DefaultOnNull, OneOrMany<_, PreferMany>)>")]
    pub address: Vec<Address>,

    // NOTE: we are not checking if this is of size 4, which is the limit in the spec
    #[serde(rename = "topics", default)]
    #[serde_as(deserialize_as = "DefaultOnNull")]
    pub topics: Vec<LogFilterInputTopic>,
}

impl LogFilterInput {
    /// Parses itself into a filter that can be applied in produced log events or to query the storage.
    pub fn parse(self, storage: &Arc<StratusStorage>) -> anyhow::Result<LogFilter, StratusError> {
        let original_input = self.clone();

        // parse point-in-time
        let (from, to) = match self.block_hash {
            Some(hash) => {
                let from_to = storage.translate_to_point_in_time(BlockFilter::Hash(hash))?;
                (from_to, from_to)
            }
            None => {
                let from = storage.translate_to_point_in_time(self.from_block.unwrap_or(BlockFilter::Latest))?;
                let to = storage.translate_to_point_in_time(self.to_block.unwrap_or(BlockFilter::Latest))?;
                (from, to)
            }
        };

        // translate point-in-time to block according to context
        let from = match from {
            PointInTime::Pending => storage.read_pending_block_header().0.number,
            PointInTime::Latest => storage.read_mined_block_number(),
            PointInTime::Past(number) => number,
        };
        let to = match to {
            PointInTime::Pending => None,
            PointInTime::Latest => None,
            PointInTime::Past(number) => Some(number),
        };

        if let Some(to_block) = to
            && to_block < from
        {
            return Err(RpcError::BlockRangeInvalid {
                actual: to_block.as_i128() - from.as_i128(),
                max: None,
            }
            .into());
        }

        Ok(LogFilter {
            from_block: from,
            to_block: to,
            addresses: self.address,
            original_input,
        })
    }
}

#[serde_as]
#[derive(DebugAsJson, Clone, Default, serde::Deserialize, serde::Serialize, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(fake::Dummy))]
// This nested type is necessary to fine-tune how we want serde to deserialize the topics field
pub struct LogFilterInputTopic(#[serde_as(deserialize_as = "OneOrMany<_, PreferMany>")] pub Vec<Option<LogTopic>>);

impl Deref for LogFilterInputTopic {
    type Target = Vec<Option<LogTopic>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_log_filter_input_with_topics() {
        use serde_plain::from_str as deser;

        let input = r#"{
            "fromBlock": "0x3F5DBCA",
            "toBlock": "latest",
            "address": [
              "0xea86da4a617b32b081a4af12e6c13ae7edf8dfc9"
            ],
            "topics": [
              [
                "0x712a5b346bb553ab14a2a2b44106991a5b94e4d44890d9aaa0f8e6b3268c502c",
                "0xc9de12e35626948d49833bbe7ac6ebe7e7d96e2d2a2e01e1eaca07830c0bf03d"
              ],
              null,
              "0x000000000000000000000000c23f832f3d9dd9492df35197f3ec0caa1cb23ce1",
              "0x453138313839353437323032343036323031373434307a495331324d4b446f36"
            ]
        }"#;

        let result: LogFilterInput = serde_json::from_str(input).unwrap();

        let expected = LogFilterInput {
            from_block: Some(deser("0x3F5DBCA").unwrap()),
            to_block: Some(deser("latest").unwrap()),
            block_hash: None,
            address: vec![deser("0xea86da4a617b32b081a4af12e6c13ae7edf8dfc9").unwrap()],
            topics: vec![
                LogFilterInputTopic(vec![
                    Some(deser("0x712a5b346bb553ab14a2a2b44106991a5b94e4d44890d9aaa0f8e6b3268c502c").unwrap()),
                    Some(deser("0xc9de12e35626948d49833bbe7ac6ebe7e7d96e2d2a2e01e1eaca07830c0bf03d").unwrap()),
                ]),
                LogFilterInputTopic(vec![None]),
                LogFilterInputTopic(vec![Some(deser("0x000000000000000000000000c23f832f3d9dd9492df35197f3ec0caa1cb23ce1").unwrap())]),
                LogFilterInputTopic(vec![Some(deser("0x453138313839353437323032343036323031373434307a495331324d4b446f36").unwrap())]),
            ],
        };

        assert_eq!(result, expected);
    }

    #[test]
    fn deserialize_log_filter_input_empty() {
        let input = "{}";
        let result: LogFilterInput = serde_json::from_str(input).unwrap();
        let expected = LogFilterInput::default();
        assert_eq!(result, expected);
    }

    #[test]
    fn parse_rejects_negative_block_range() {
        use crate::eth::primitives::BlockNumber;

        let storage = Arc::new(StratusStorage::new_test().unwrap());

        let input = LogFilterInput {
            from_block: Some(BlockFilter::Number(BlockNumber::from(10u32))),
            to_block: Some(BlockFilter::Number(BlockNumber::from(5u32))),
            ..LogFilterInput::default()
        };

        let err = input.parse(&storage).unwrap_err();
        match err {
            StratusError::RPC(RpcError::BlockRangeInvalid { actual, max }) => {
                // to (5) - from (10) = -5
                assert_eq!(actual, -5_i128);
                assert_eq!(max, None);
            }
            other => panic!("expected BlockRangeInvalid, got {other:?}"),
        }
    }

    #[test]
    fn parse_accepts_valid_block_range() {
        use crate::eth::primitives::BlockNumber;

        let storage = Arc::new(StratusStorage::new_test().unwrap());

        let input = LogFilterInput {
            from_block: Some(BlockFilter::Number(BlockNumber::from(5u32))),
            to_block: Some(BlockFilter::Number(BlockNumber::from(10u32))),
            ..LogFilterInput::default()
        };

        let filter = input.parse(&storage).unwrap();
        assert_eq!(filter.from_block, BlockNumber::from(5u32));
        assert_eq!(filter.to_block, Some(BlockNumber::from(10u32)));
    }
}
