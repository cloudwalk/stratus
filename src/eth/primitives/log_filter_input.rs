//! Log Filter Input Module
//!
//! Manages the input structure for Ethereum log filtering, particularly for
//! JSON-RPC methods like `eth_getLogs` and `eth_subscribe`. This module
//! defines how filters are inputted, parsed, and translated into log filters.
//! It's a critical interface for users or applications specifying criteria for
//! log entries they are interested in retrieving or monitoring.

use std::sync::Arc;

use itertools::Itertools;
use serde_with::formats::PreferMany;
use serde_with::serde_as;
use serde_with::OneOrMany;

use crate::eth::primitives::Address;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogFilterTopicCombination;
use crate::eth::primitives::LogTopic;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::storage::EthStorage;

/// JSON-RPC input used in methods like `eth_getLogs` and `eth_subscribe`.
#[serde_as]
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct LogFilterInput {
    #[serde(rename = "fromBlock", default)]
    pub from_block: Option<BlockSelection>,

    #[serde(rename = "toBlock", default)]
    pub to_block: Option<BlockSelection>,

    #[serde(rename = "blockHash", default)]
    pub block_hash: Option<Hash>,

    #[serde(rename = "address", default)]
    #[serde_as(deserialize_as = "OneOrMany<_, PreferMany>")]
    pub address: Vec<Address>,

    #[serde(rename = "topics", default)]
    #[serde_as(deserialize_as = "OneOrMany<_, PreferMany>")]
    pub topics: Vec<Vec<Option<LogTopic>>>,
}

impl LogFilterInput {
    /// Parses itself into a filter that can be applied in produced log events or to query the storage.
    pub fn parse(self, storage: &Arc<dyn EthStorage>) -> anyhow::Result<LogFilter> {
        // parse point-in-time
        let (from, to) = match self.block_hash {
            Some(hash) => {
                let from_to = storage.translate_to_point_in_time(&BlockSelection::Hash(hash))?;
                (from_to.clone(), from_to)
            }
            None => {
                let from = storage.translate_to_point_in_time(&self.from_block.unwrap_or(BlockSelection::Latest))?;
                let to = storage.translate_to_point_in_time(&self.to_block.unwrap_or(BlockSelection::Latest))?;
                (from, to)
            }
        };

        // translate point-in-time to block according to context
        let from = match from {
            StoragePointInTime::Present => storage.read_current_block_number()?,
            StoragePointInTime::Past(number) => number,
        };
        let to = match to {
            StoragePointInTime::Present => None,
            StoragePointInTime::Past(number) => Some(number),
        };

        let topics: Vec<LogFilterTopicCombination> = self
            .topics
            .into_iter()
            .map(|topics| {
                topics
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, topic)| topic.map(|topic| (index, topic)))
                    .collect_vec()
                    .into()
            })
            .collect_vec();

        Ok(LogFilter {
            from_block: from,
            to_block: to,
            addresses: self.address,
            topics_combinations: topics,
        })
    }
}
