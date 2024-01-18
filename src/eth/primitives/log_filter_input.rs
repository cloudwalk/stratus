use std::sync::Arc;

use crate::eth::primitives::Address;
use crate::eth::primitives::BlockSelection;
use crate::eth::primitives::Hash;
use crate::eth::primitives::LogFilter;
use crate::eth::primitives::LogTopic;
use crate::eth::primitives::StoragePointInTime;
use crate::eth::storage::EthStorage;
use crate::eth::EthError;

/// JSON-RPC input used in methods like `eth_getLogs` and `eth_subscribe`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct LogFilterInput {
    #[serde(rename = "fromBlock", default)]
    pub from_block: Option<BlockSelection>,

    #[serde(rename = "toBlock", default)]
    pub to_block: Option<BlockSelection>,

    #[serde(rename = "blockHash", default)]
    pub block_hash: Option<Hash>,

    #[serde(default)]
    pub address: Option<Address>,

    #[serde(default)]
    pub topics: Vec<Option<LogTopic>>,
}

impl LogFilterInput {
    /// Parses itself into a filter that can be applied in produced log events or to query the storage.
    pub fn parse(self, storage: &Arc<dyn EthStorage>) -> Result<LogFilter, EthError> {
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

        Ok(LogFilter {
            from_block: from,
            to_block: to,
            address: self.address,
            topics: self
                .topics
                .into_iter()
                .enumerate()
                .filter_map(|(index, topic)| topic.map(|topic| (index, topic)))
                .collect(),
        })
    }
}
