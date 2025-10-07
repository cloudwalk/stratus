use display_json::DebugAsJson;

use crate::alias::AlloyLog;
use crate::alias::AlloyLogData;
use crate::alias::AlloyLogPrimitive;
use crate::alias::RevmLog;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::LogTopic;
use crate::eth::primitives::UnixTime;

/// Log is an event emitted by the EVM during contract execution.
#[derive(DebugAsJson, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct Log {
    /// Address that emitted the log.
    pub address: Address,

    /// Topics (0 to 4 positions) describing the log.
    pub topic0: Option<LogTopic>,
    pub topic1: Option<LogTopic>,
    pub topic2: Option<LogTopic>,
    pub topic3: Option<LogTopic>,

    /// Additional data.
    pub data: Bytes,

    /// log index (Some if mined)
    pub index: Option<Index>,
}

impl Log {
    /// Returns all topics in the log.
    pub fn topics(&self) -> [Option<LogTopic>; 4] {
        [self.topic0, self.topic1, self.topic2, self.topic3]
    }

    /// Returns all non-empty topics in the log.
    pub fn topics_non_empty(&self) -> Vec<LogTopic> {
        self.topics().into_iter().flatten().collect()
    }

    pub fn to_alloy_log(
        self,
        block_hash: Hash,
        block_number: BlockNumber,
        block_timestamp: UnixTime,
        transaction_hash: Hash,
        transaction_index: Index,
        log_index: Index,
    ) -> AlloyLog {
        AlloyLog {
            inner: AlloyLogPrimitive {
                address: self.address.into(),
                // Using new_unchecked is safe because topics_non_empty() guarantees ≤ 4 topics
                data: AlloyLogData::new_unchecked(self.topics_non_empty().into_iter().map(Into::into).collect(), self.data.into()),
            },
            block_hash: Some(block_hash.into()),
            block_number: Some(block_number.as_u64()),
            block_timestamp: Some(*block_timestamp),
            transaction_hash: Some(transaction_hash.into()),
            transaction_index: Some(transaction_index.into()),
            log_index: Some(log_index.into()),
            removed: false,
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// ----------------------------------------------------------------------------
impl From<RevmLog> for Log {
    fn from(value: RevmLog) -> Self {
        let (topics, data) = value.data.split();
        let topics_len = topics.len();

        let mut log = Self {
            address: value.address.into(),
            data: data.into(),
            ..Default::default()
        };

        // you may not like it but this is what peak performance looks like
        match topics_len {
            4 => {
                log.topic0 = Some(topics[0].into());
                log.topic1 = Some(topics[1].into());
                log.topic2 = Some(topics[2].into());
                log.topic3 = Some(topics[3].into());
            }
            3 => {
                log.topic0 = Some(topics[0].into());
                log.topic1 = Some(topics[1].into());
                log.topic2 = Some(topics[2].into());
            }
            2 => {
                log.topic0 = Some(topics[0].into());
                log.topic1 = Some(topics[1].into());
            }
            1 => {
                log.topic0 = Some(topics[0].into());
            }
            _ => {}
        }

        log
    }
}

impl From<AlloyLog> for Log {
    fn from(value: AlloyLog) -> Self {
        let topics = value.inner.topics().to_vec();
        let topics_len = topics.len();

        let mut log = Self {
            address: value.inner.address.into(),
            data: value.inner.data.data.into(),
            ..Default::default()
        };

        // you may not like it but this is what peak performance looks like
        match topics_len {
            4 => {
                log.topic0 = Some(topics[0].into());
                log.topic1 = Some(topics[1].into());
                log.topic2 = Some(topics[2].into());
                log.topic3 = Some(topics[3].into());
            }
            3 => {
                log.topic0 = Some(topics[0].into());
                log.topic1 = Some(topics[1].into());
                log.topic2 = Some(topics[2].into());
            }
            2 => {
                log.topic0 = Some(topics[0].into());
                log.topic1 = Some(topics[1].into());
            }
            1 => {
                log.topic0 = Some(topics[0].into());
            }
            _ => {}
        }

        log
    }
}
