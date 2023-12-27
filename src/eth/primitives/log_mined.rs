use ethers_core::types::Log as EthersLog;
use itertools::Itertools;

use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Log;

/// Log that was emitted by the EVM and added to a block.
#[derive(Debug, Clone)]
pub struct LogMined {
    /// Original log emitted by the EVM.
    pub log: Log,

    /// Hash of the transaction that emitted this log.
    pub transaction_hash: Hash,

    /// Position of the transaction that emitted this log inside the block.
    pub transaction_index: usize,

    /// Position of the log inside the block.
    pub log_index: usize,

    /// Block number where the log was mined.
    pub block_number: BlockNumber,

    /// Block hash where the log was mined.
    pub block_hash: Hash,
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<LogMined> for EthersLog {
    fn from(value: LogMined) -> Self {
        Self {
            // log
            address: value.log.address.into(),
            topics: value.log.topics.into_iter().map_into().collect(),
            data: value.log.data.into(),
            log_index: Some(value.log_index.into()),
            removed: Some(false),
            log_type: None, // TODO: what is this?,

            // block / transaction
            block_hash: Some(value.block_hash.into()),
            block_number: Some(value.block_number.into()),
            transaction_hash: Some(value.transaction_hash.into()),
            transaction_index: Some(value.transaction_index.into()),
            transaction_log_index: None, // TODO: what is this?
        }
    }
}
