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
        self.topics_array().into_iter().flatten().collect()
    }

    pub fn topics_array(&self) -> [Option<LogTopic>; 4] {
        [self.topic0, self.topic1, self.topic2, self.topic3]
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

impl From<EthersLog> for Log {
    fn from(value: EthersLog) -> Self {
        let topics = value.topics;
        let topics_len = topics.len();

        let mut log = Self {
            address: value.address.into(),
            data: value.data.into(),
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
