use display_json::DebugAsJson;

use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::LogFilterInput;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::logs_bloom::LogsBloom;
use crate::ext::not;

#[derive(Clone, DebugAsJson, serde::Serialize, Eq, Hash, PartialEq)]
#[cfg_attr(test, derive(serde::Deserialize, fake::Dummy))]
#[cfg_attr(test, derive(Default))]
pub struct LogFilter {
    pub from_block: BlockNumber,
    pub to_block: Option<BlockNumber>,
    pub addresses: Vec<Address>,

    /// Original payload received via RPC.
    #[cfg_attr(not(test), serde(skip))]
    pub original_input: LogFilterInput,
}

impl LogFilter {
    /// Checks if a bloom filter might contain logs that match this filter.
    /// 
    /// This is a quick check that can be used to skip blocks that definitely don't contain matching logs.
    /// Returns true if the bloom filter might contain matching logs, false if it definitely doesn't.
    #[cfg(feature = "dev")]
    pub fn may_contain_matching_logs(&self, bloom: &LogsBloom) -> bool {
        // If no addresses in filter, any block with logs might match
        if self.addresses.is_empty() {
            // If there are topics, we still need to check them against the bloom
            let topics_empty = self.original_input.topics.is_empty();
            if topics_empty {
                // No addresses and no topics means any block with logs could match
                return true;
            }
        } else {
            // Check if any of the addresses in the filter are in the bloom
            let mut any_address_matches = false;
            for address in &self.addresses {
                if bloom.contains_input(ethereum_types::BloomInput::Raw(address.as_ref())) {
                    any_address_matches = true;
                    break;
                }
            }
            
            // If none of the addresses match, the block definitely doesn't contain matching logs
            if !any_address_matches {
                return false;
            }
        }

        // Check topics
        for filter_topic in &self.original_input.topics {
            // If the topic filter is empty or contains None, it matches anything
            if filter_topic.is_empty() || filter_topic.contains(&None) {
                continue;
            }
            
            // Check if any of the topics in this position are in the bloom
            let mut any_topic_matches = false;
            for topic in filter_topic.iter().flatten() {
                if bloom.contains_input(ethereum_types::BloomInput::Raw(topic.as_ref())) {
                    any_topic_matches = true;
                    break;
                }
            }
            
            // If none of the topics in this position match, the block definitely doesn't contain matching logs
            if !any_topic_matches {
                return false;
            }
        }
        
        // If we get here, the block might contain matching logs
        true
    }

    /// Checks if a log matches the filter.
    pub fn matches(&self, log: &LogMined) -> bool {
        // filter block range
        if log.block_number < self.from_block {
            return false;
        }
        if self.to_block.as_ref().is_some_and(|to_block| log.block_number > *to_block) {
            return false;
        }

        // filter address
        let has_addresses = not(self.addresses.is_empty());
        if has_addresses && not(self.addresses.contains(&log.address())) {
            return false;
        }

        let filter_topics = &self.original_input.topics;
        let log_topics = log.log.topics();

        // (https://ethereum.org/en/developers/docs/apis/json-rpc/#eth_getlogs)
        // Matching rules for filtering topics in `eth_getLogs`:
        //
        // - `[]`: anything
        // - `[A]`: A in first position (and anything after)
        // - `[null, B]`: anything in first position AND B in second position (and anything after)
        // - `[A, B]`: A in first position AND B in second position (and anything after)
        // - `[[A, B], [A, B]]`: (A OR B) in first position AND (A OR B) in second position (and anything after)
        //
        // And from
        //
        // But it seems to leave the following unspecified:
        //
        // - `[[A, B, null]]`: ?
        //   - `null` in nested array with other non-null items
        // - `[[]]`: ?
        //   - `[]` as an inner array alone, after, or before other elements
        //
        // In doubt of what to do, this implementation will:
        //
        // - Treat `[[null]]` as `[null]`, that is, match anything for that index.
        // - Treat `[[]]` as `[null]` (same as above), match anything for that index.

        // filter field missing, set to `null` or equal to `[]`
        if filter_topics.is_empty() {
            return true;
        }

        for (log_topic, filter_topic) in log_topics.into_iter().zip(filter_topics) {
            // the unspecified nested `[[]]`
            if filter_topic.is_empty() {
                continue; // match anything
            }
            // `[null]` and `[[null]]` (due to how this is deserialized)
            if filter_topic.contains(&None) {
                continue; // match anything
            }
            // `[A, ..]` ,`[[A], ..]` and `[[A, B, C, ..], ..]` (due to how this is deserialized)
            if !filter_topic.contains(&log_topic) {
                return false; // not included in OR filter, filtered out
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use itertools::Itertools;

    use super::*;
    use crate::eth::primitives::Log;
    use crate::eth::primitives::LogFilterInputTopic;
    use crate::eth::primitives::LogTopic;
    use crate::eth::storage::StratusStorage;
    use crate::utils::test_utils::fake_first;
    use crate::utils::test_utils::fake_list;

    fn build_filter(addresses: Vec<Address>, topics_nested: Vec<Vec<Option<LogTopic>>>) -> LogFilter {
        let topics_map = |topics: Vec<Option<LogTopic>>| LogFilterInputTopic(topics.into_iter().collect());

        let storage = StratusStorage::new_test().unwrap();

        LogFilterInput {
            address: addresses,
            topics: topics_nested.into_iter().map(topics_map).collect(),
            ..LogFilterInput::default()
        }
        .parse(&Arc::new(storage))
        .unwrap()
    }

    fn log_with_topics(topics: [Option<LogTopic>; 4]) -> LogMined {
        let mut log_mined = fake_first::<LogMined>();
        log_mined.log = Log {
            topic0: topics[0],
            topic1: topics[1],
            topic2: topics[2],
            topic3: topics[3],
            ..log_mined.log
        };
        log_mined
    }

    fn log_with_address(address: Address) -> LogMined {
        let mut log_mined = fake_first::<LogMined>();
        log_mined.log = Log { address, ..log_mined.log };
        log_mined
    }

    #[test]
    fn log_filtering_by_topic() {
        let topics = fake_list::<LogTopic>(8).into_iter().map(Some).collect_vec();

        let filter = build_filter(
            vec![],
            vec![
                vec![topics[1], topics[2], topics[3]],
                vec![None],
                vec![topics[4], topics[5]],
                vec![topics[6], topics[7]],
            ],
        );

        assert!(filter.matches(&log_with_topics([topics[1], None, topics[4], topics[6]])));
        assert!(filter.matches(&log_with_topics([topics[2], None, topics[4], topics[6]])));
        assert!(filter.matches(&log_with_topics([topics[3], None, topics[4], topics[6]])));
        assert!(filter.matches(&log_with_topics([topics[3], None, topics[5], topics[6]])));
        assert!(filter.matches(&log_with_topics([topics[3], topics[0], topics[5], topics[7]])));
        assert!(filter.matches(&log_with_topics([topics[1], topics[2], topics[4], topics[6]])));
        assert!(filter.matches(&log_with_topics([topics[2], topics[4], topics[4], topics[7]])));
        assert!(filter.matches(&log_with_topics([topics[2], topics[7], topics[5], topics[6]])));

        assert!(not(filter.matches(&log_with_topics([None, None, None, None]))));
        assert!(not(filter.matches(&log_with_topics([topics[0], None, None, None]))));
        assert!(not(filter.matches(&log_with_topics([None, topics[0], None, None]))));
        assert!(not(filter.matches(&log_with_topics([None, None, topics[0], None]))));
        assert!(not(filter.matches(&log_with_topics([None, None, None, topics[0]]))));
        assert!(not(filter.matches(&log_with_topics([topics[2], topics[2], topics[4], topics[0]]))));
        assert!(not(filter.matches(&log_with_topics([topics[3], None, topics[5], None]))));
        assert!(not(filter.matches(&log_with_topics([topics[2], topics[4], None, topics[6]]))));
        assert!(not(filter.matches(&log_with_topics([None, topics[0], topics[4], topics[6]]))));
        assert!(not(filter.matches(&log_with_topics([topics[3], topics[0], topics[4], None]))));

        let filter = build_filter(vec![], vec![vec![None], vec![topics[1], topics[2]]]);

        assert!(filter.matches(&log_with_topics([topics[1], topics[1], topics[1], topics[1]])));
        assert!(filter.matches(&log_with_topics([topics[1], topics[1], topics[1], None])));
        assert!(filter.matches(&log_with_topics([topics[1], topics[1], None, topics[1]])));
        assert!(filter.matches(&log_with_topics([None, topics[1], topics[1], topics[1]])));
        assert!(filter.matches(&log_with_topics([topics[0], topics[1], None, topics[2]])));
        assert!(filter.matches(&log_with_topics([None, topics[2], None, topics[1]])));

        assert!(not(filter.matches(&log_with_topics([topics[1], None, topics[1], topics[1]]))));
        assert!(not(filter.matches(&log_with_topics([topics[1], None, None, None]))));
        assert!(not(filter.matches(&log_with_topics([topics[1], topics[3], None, None]))));
        assert!(not(filter.matches(&log_with_topics([None, topics[3], None, None]))));
        assert!(not(filter.matches(&log_with_topics([None, None, None, None]))));
    }

    #[test]
    fn log_filtering_by_address() {
        let addresses = fake_list::<Address>(4);

        let filter = build_filter(vec![addresses[1], addresses[2]], vec![]);

        assert!(filter.matches(&log_with_address(addresses[1])));
        assert!(filter.matches(&log_with_address(addresses[2])));

        assert!(not(filter.matches(&log_with_address(addresses[0]))));
        assert!(not(filter.matches(&log_with_address(addresses[3]))));

        let filter = build_filter(vec![], vec![]);

        assert!(filter.matches(&log_with_address(addresses[0])));
        assert!(filter.matches(&log_with_address(addresses[1])));
        assert!(filter.matches(&log_with_address(addresses[2])));
        assert!(filter.matches(&log_with_address(addresses[3])));
    }

    #[test]
    #[cfg(feature = "dev")]
    fn test_may_contain_matching_logs() {
        use hex_literal::hex;
        
        let addresses = fake_list::<Address>(4);
        let topics = fake_list::<LogTopic>(8).into_iter().map(Some).collect_vec();
        
        // Create a bloom filter with specific logs
        let log1 = Log {
            address: addresses[1],
            topic0: topics[1],
            topic1: topics[2],
            topic2: topics[3],
            topic3: None,
            data: hex!("0000000000000000000000000000000000000000000000000000000005f5e100").as_ref().into(),
        };
        
        let log2 = Log {
            address: addresses[2],
            topic0: topics[4],
            topic1: topics[5],
            topic2: None,
            topic3: None,
            data: hex!("0000000000000000000000000000000000000000000000000000000000004dca").as_ref().into(),
        };
        
        let mut bloom = LogsBloom::default();
        bloom.accrue_log(&log1);
        bloom.accrue_log(&log2);
        
        // Test filter that should match
        let filter1 = build_filter(
            vec![addresses[1]],
            vec![vec![topics[1]], vec![topics[2]]],
        );
        assert!(filter1.may_contain_matching_logs(&bloom));
        
        // Test filter with address that should match
        let filter2 = build_filter(
            vec![addresses[2]],
            vec![],
        );
        assert!(filter2.may_contain_matching_logs(&bloom));
        
        // Test filter with address that should not match
        let filter3 = build_filter(
            vec![addresses[0]],
            vec![],
        );
        assert!(!filter3.may_contain_matching_logs(&bloom));
        
        // Test filter with topics that should match
        let filter4 = build_filter(
            vec![],
            vec![vec![topics[4]], vec![topics[5]]],
        );
        assert!(filter4.may_contain_matching_logs(&bloom));
        
        // Test filter with topic that should not match
        let filter5 = build_filter(
            vec![],
            vec![vec![topics[0]]],
        );
        assert!(!filter5.may_contain_matching_logs(&bloom));
        
        // Test filter with matching address but non-matching topic
        let filter6 = build_filter(
            vec![addresses[1]],
            vec![vec![topics[0]]],
        );
        assert!(!filter6.may_contain_matching_logs(&bloom));
        
        // Test filter with empty criteria (should match any bloom)
        let filter7 = build_filter(
            vec![],
            vec![],
        );
        assert!(filter7.may_contain_matching_logs(&bloom));
        
        // Test filter with None topic (should match any bloom in that position)
        let filter8 = build_filter(
            vec![addresses[1]],
            vec![vec![None], vec![topics[2]]],
        );
        assert!(filter8.may_contain_matching_logs(&bloom));
    }
}
