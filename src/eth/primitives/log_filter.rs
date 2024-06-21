use display_json::DebugAsJson;

use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::LogFilterInput;
use crate::eth::primitives::LogMined;
use crate::ext::not;

#[derive(DebugAsJson, serde::Serialize)]
#[cfg_attr(test, derive(Default))]
pub struct LogFilter {
    pub from_block: BlockNumber,
    pub to_block: Option<BlockNumber>,
    pub addresses: Vec<Address>,

    /// Original payload received via RPC.
    #[serde(skip)]
    pub original_input: LogFilterInput,
}

impl LogFilter {
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
        if has_addresses && not(self.addresses.contains(log.address())) {
            return false;
        }

        let filter_topics = &self.original_input.topics;
        let log_topics = log.log.topics_array();

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

    // TODO: consider moving these 3 functions to the file that defined the returned type

    fn build_filter(addresses: Vec<Address>, topics_nested: Vec<Vec<Option<LogTopic>>>) -> LogFilter {
        let topics_map = |topics: Vec<Option<LogTopic>>| LogFilterInputTopic(topics.into_iter().collect());

        LogFilterInput {
            address: addresses,
            topics: topics_nested.into_iter().map(topics_map).collect(),
            ..LogFilterInput::default()
        }
        .parse(&Arc::new(StratusStorage::mock_new()))
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
}
