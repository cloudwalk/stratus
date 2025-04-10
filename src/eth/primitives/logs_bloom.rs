use std::ops::Deref;
use std::ops::DerefMut;

use ethereum_types::Bloom;

use crate::alias::AlloyBloom;
use crate::eth::primitives::Log;
use crate::gen_newtype_from;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(transparent)]
pub struct LogsBloom(pub Bloom);

impl LogsBloom {
    pub fn accrue_log(&mut self, log: &Log) {
        self.accrue(ethereum_types::BloomInput::Raw(log.address.as_ref()));
        for topic in log.topics_non_empty() {
            self.accrue(ethereum_types::BloomInput::Raw(topic.as_ref()));
        }
    }

    /// Checks if this bloom filter might contain logs that match the given filter.
    ///
    /// This is a quick check that can be used to skip blocks that definitely don't contain matching logs.
    /// Returns true if the bloom filter might contain matching logs, false if it definitely doesn't.
    pub fn matches_filter(&self, filter: &crate::eth::primitives::LogFilter) -> bool {
        tracing::trace!(
            addresses_len = filter.addresses.len(),
            topics_len = filter.original_input.topics.len(),
            "Checking if bloom filter matches log filter"
        );

        // If no addresses in filter, any block with logs might match
        if filter.addresses.is_empty() {
            // If there are topics, we still need to check them against the bloom
            let topics_empty = filter.original_input.topics.is_empty();
            if topics_empty {
                // No addresses and no topics means any block with logs could match
                tracing::trace!("No addresses and no topics in filter, bloom may match");
                return true;
            }
        } else {
            // Check if any of the addresses in the filter are in the bloom
            let mut any_address_matches = false;
            for (i, address) in filter.addresses.iter().enumerate() {
                let matches = self.contains_input(ethereum_types::BloomInput::Raw(address.as_ref()));
                tracing::trace!(
                    address_idx = i,
                    address = ?address,
                    matches,
                    "Checking address against bloom"
                );
                if matches {
                    any_address_matches = true;
                    break;
                }
            }

            // If none of the addresses match, the block definitely doesn't contain matching logs
            if !any_address_matches {
                tracing::trace!("No addresses in filter match bloom, definitely doesn't match");
                return false;
            }
            tracing::trace!("At least one address in filter matches bloom");
        }

        // Check topics
        for (i, filter_topic) in filter.original_input.topics.iter().enumerate() {
            // If the topic filter is empty or contains None, it matches anything
            if filter_topic.is_empty() || filter_topic.contains(&None) {
                tracing::trace!(
                    topic_idx = i,
                    is_empty = filter_topic.is_empty(),
                    contains_none = filter_topic.contains(&None),
                    "Topic position matches anything (empty or contains None)"
                );
                continue;
            }

            // Check if any of the topics in this position are in the bloom
            let mut any_topic_matches = false;
            for (j, topic_opt) in filter_topic.iter().enumerate() {
                if let Some(topic) = topic_opt {
                    let matches = self.contains_input(ethereum_types::BloomInput::Raw(topic.as_ref()));
                    tracing::trace!(
                        topic_idx = i,
                        topic_option_idx = j,
                        topic = ?topic,
                        matches,
                        "Checking topic against bloom"
                    );
                    if matches {
                        any_topic_matches = true;
                        break;
                    }
                } else {
                    tracing::trace!(topic_idx = i, topic_option_idx = j, "Topic is None, matches anything");
                    // None matches anything, already handled by the check above
                }
            }

            // If none of the topics in this position match, the block definitely doesn't contain matching logs
            if !any_topic_matches {
                tracing::trace!(topic_idx = i, "No topics in this position match bloom, definitely doesn't match");
                return false;
            }
            tracing::trace!(topic_idx = i, "At least one topic in this position matches bloom");
        }

        // If we get here, the block might contain matching logs
        tracing::trace!("Bloom filter might contain matching logs");
        true
    }
}

impl Deref for LogsBloom {
    type Target = Bloom;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for LogsBloom {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
gen_newtype_from!(self = LogsBloom, other = [u8; 256], Bloom);

impl From<AlloyBloom> for LogsBloom {
    fn from(value: AlloyBloom) -> Self {
        let bytes: [u8; 256] = *value.0;
        Self(Bloom::from(bytes))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------

impl From<LogsBloom> for Bloom {
    fn from(value: LogsBloom) -> Self {
        value.0
    }
}

impl From<LogsBloom> for [u8; 256] {
    fn from(value: LogsBloom) -> Self {
        value.0 .0
    }
}

impl From<LogsBloom> for AlloyBloom {
    fn from(value: LogsBloom) -> Self {
        AlloyBloom::from(value.0 .0)
    }
}

#[cfg(test)]
mod tests {
    use hex_literal::hex;

    use super::*;

    #[test]
    fn compute_bloom() {
        let log1 = Log {
            address: hex!("c6d1efd908ef6b69da0749600f553923c465c812").into(),
            topic0: Some(hex!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef").into()),
            topic1: Some(hex!("000000000000000000000000d1ff9b395856e5a6810f626eca09d61d34fce3b8").into()),
            topic2: Some(hex!("000000000000000000000000081d2c5b26db6f6e0944e4725b3b61b26e25dd8a").into()),
            topic3: None,
            data: hex!("0000000000000000000000000000000000000000000000000000000005f5e100").as_ref().into(),
        };
        let log2 = Log {
            address: hex!("b1f571b3254c99a0a562124738f0193de2b2b2a9").into(),
            topic0: Some(hex!("8d995e7fbf7a5ef41cee9e6936368925d88e07af89306bb78a698551562e683c").into()),
            topic1: Some(hex!("000000000000000000000000081d2c5b26db6f6e0944e4725b3b61b26e25dd8a").into()),
            topic2: None,
            topic3: None,
            data: hex!(
                "0000000000000000000000000000000000000000000000000000000000004dca00000000\
                0000000000000000000000000000000000000000000000030c9281f0"
            )
            .as_ref()
            .into(),
        };
        let mut bloom = LogsBloom::default();
        bloom.accrue_log(&log1);
        bloom.accrue_log(&log2);

        let expected: LogsBloom = hex!(
            "000000000400000000000000000000000000000000000000000000000000\
        00000000000000000000000000000000000000080000000000000000000000000000000000000000000000000008\
        00008400202000000000002000000000000000000000000000000010000000000000000000000000040000000000\
        00100000000000000000000000000000000000000000000000000000000000000000000000001000000000000100\
        00000000000000000000000000000000000000000000080000000002000000000000000000000000000000002000\
        000000440000000000000000000000000000000000000000000000000000000000000000000000000000"
        )
        .into();

        assert_eq!(bloom, expected);
    }
}
