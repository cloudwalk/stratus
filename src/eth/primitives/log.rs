//! Log Module
//!
//! Handles Ethereum log entries, which are key components of Ethereum's event
//! system. Logs are emitted by smart contracts and are crucial for tracking
//! contract events. This module defines the structure of logs, including the
//! emitting address, topics, and additional data. It also provides conversion
//! functions to translate between internal and external log representations.

use ethers_core::types::Log as EthersLog;
use revm::primitives::Log as RevmLog;

use crate::eth::primitives::Address;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::LogTopic;

/// Log is an event emitted by the EVM during contract execution.
#[derive(Debug, Clone, Default, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
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
}

impl Log {
    pub fn topics(&self) -> Vec<LogTopic> {
        [self.topic0, self.topic1, self.topic2, self.topic3].into_iter().flatten().collect()
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

        if topics_len == 4 {
            log.topic0 = Some(topics[0].into());
            log.topic1 = Some(topics[1].into());
            log.topic2 = Some(topics[2].into());
            log.topic3 = Some(topics[3].into());
        }
        if topics_len == 3 {
            log.topic0 = Some(topics[0].into());
            log.topic1 = Some(topics[1].into());
            log.topic2 = Some(topics[2].into());
        }
        if topics_len == 2 {
            log.topic0 = Some(topics[0].into());
            log.topic1 = Some(topics[1].into());
        }
        if topics_len == 1 {
            log.topic0 = Some(topics[0].into());
        }

        log
    }
}

impl From<EthersLog> for Log {
    fn from(value: EthersLog) -> Self {
        let topics = value.topics;
        let topics_len = topics.len();

        let mut log = Self {
            address: value.address.into(),
            data: value.data.into(),
            ..Default::default()
        };

        if topics_len == 4 {
            log.topic0 = Some(topics[0].into());
            log.topic1 = Some(topics[1].into());
            log.topic2 = Some(topics[2].into());
            log.topic3 = Some(topics[3].into());
        }
        if topics_len == 3 {
            log.topic0 = Some(topics[0].into());
            log.topic1 = Some(topics[1].into());
            log.topic2 = Some(topics[2].into());
        }
        if topics_len == 2 {
            log.topic0 = Some(topics[0].into());
            log.topic1 = Some(topics[1].into());
        }
        if topics_len == 1 {
            log.topic0 = Some(topics[0].into());
        }

        log
    }
}
