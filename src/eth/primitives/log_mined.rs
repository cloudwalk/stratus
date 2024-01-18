//! Log Mined Module
//!
//! Manages Ethereum logs that have been included in mined blocks. This module
//! extends the basic log structure to include context of the block and
//! transaction in which the log was mined, such as the block number, block
//! hash, and transaction index. It is essential for associating logs with
//! their blockchain context.

use ethers_core::types::Log as EthersLog;
use itertools::Itertools;
use jsonrpsee::SubscriptionMessage;
use serde_json::Value as JsonValue;

use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Log;
use crate::eth::primitives::LogTopic;
use crate::eth::primitives::Index;

/// Log that was emitted by the EVM and added to a block.
#[derive(Debug, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
pub struct LogMined {
    /// Original log emitted by the EVM.
    pub log: Log,

    /// Hash of the transaction that emitted this log.
    pub transaction_hash: Hash,

    /// Position of the transaction that emitted this log inside the block.
    pub transaction_index: Index,

    /// Position of the log inside the block.
    pub log_index: Index,

    /// Block number where the log was mined.
    pub block_number: BlockNumber,

    /// Block hash where the log was mined.
    pub block_hash: Hash,
}

impl LogMined {
    /// Returns the address that emitted the log.
    pub fn address(&self) -> &Address {
        &self.log.address
    }

    /// Returns the topics emitted in the log.
    pub fn topics(&self) -> &[LogTopic] {
        &self.log.topics
    }

    /// Serializes itself to JSON-RPC log format.
    pub fn to_json_rpc_log(self) -> JsonValue {
        let json_rpc_format: EthersLog = self.into();
        serde_json::to_value(json_rpc_format).unwrap()
    }
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

impl From<LogMined> for SubscriptionMessage {
    fn from(value: LogMined) -> Self {
        let ethers_log = Into::<EthersLog>::into(value);
        Self::from_json(&ethers_log).unwrap()
    }
}
