use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::LogMined;
use crate::eth::primitives::LogTopic;
use crate::ext::not;

#[derive(Clone, Debug)]
pub struct LogFilter {
    pub from_block: BlockNumber,
    pub to_block: Option<BlockNumber>,
    pub addresses: Vec<Address>,
    pub topics: Vec<(usize, LogTopic)>,
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

        // filter addres
        if not(self.addresses.contains(log.address())) {
            return false;
        }

        // filter topics
        let log_topics = log.topics();
        for (filter_topic_index, filter_topic) in self.topics.iter() {
            if log_topics.get(*filter_topic_index).is_some_and(|topic| topic != filter_topic) {
                return false;
            }
        }

        true
    }
}
