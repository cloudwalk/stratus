use display_json::DebugAsJson;
use jsonrpsee::SubscriptionMessage;

use crate::alias::AlloyLog;
use crate::alias::AlloyLogData;
use crate::alias::AlloyLogPrimitive;
use crate::alias::JsonValue;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::Log;
use crate::ext::to_json_value;

/// Log that was emitted by the EVM and added to a block.
#[derive(DebugAsJson, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(fake::Dummy))]
pub struct LogMessage {
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

impl LogMessage {
    /// Serializes itself to JSON-RPC log format.
    pub fn to_json_rpc_log(self) -> JsonValue {
        let alloy_log: AlloyLog = self.into();
        to_json_value(alloy_log)
    }

    pub fn from_parts(
        log: Log,
        block_number: BlockNumber,
        block_hash: Hash,
        transaction_hash: Hash,
        log_index: Index,
        transaction_index: Index,
    ) -> Self {
        Self {
            log,
            transaction_hash,
            transaction_index,
            log_index,
            block_number,
            block_hash,
        }
    }
}

impl From<LogMessage> for AlloyLog {
    fn from(value: LogMessage) -> Self {
        Self {
            inner: AlloyLogPrimitive {
                address: value.log.address.into(),
                // Using new_unchecked is safe because topics_non_empty() guarantees ≤ 4 topics
                data: AlloyLogData::new_unchecked(value.log.topics_non_empty().into_iter().map(Into::into).collect(), value.log.data.into()),
            },
            block_hash: Some(value.block_hash.into()),
            block_number: Some(value.block_number.as_u64()),
            block_timestamp: None,
            transaction_hash: Some(value.transaction_hash.into()),
            transaction_index: Some(value.transaction_index.into()),
            log_index: Some(value.log_index.into()),
            removed: false,
        }
    }
}

impl TryFrom<LogMessage> for SubscriptionMessage {
    type Error = serde_json::Error;

    fn try_from(value: LogMessage) -> Result<Self, Self::Error> {
        Ok(serde_json::value::RawValue::from_string(serde_json::to_string(&Into::<AlloyLog>::into(value))?)?.into())
    }
}
