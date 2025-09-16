use display_json::DebugAsJson;
use jsonrpsee::SubscriptionMessage;

use super::TransactionExecution;
use crate::alias::AlloyLog;
use crate::alias::AlloyLogData;
use crate::alias::AlloyLogPrimitive;
use crate::alias::JsonValue;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Hash;
use crate::eth::primitives::Index;
use crate::eth::primitives::Log;
use crate::eth::primitives::LogTopic;
use crate::ext::to_json_value;

/// Log that was emitted by the EVM and added to a block.
#[derive(DebugAsJson, Clone, PartialEq, Eq, fake::Dummy, serde::Serialize, serde::Deserialize)]
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
    pub fn address(&self) -> Address {
        self.log.address
    }

    /// Returns all non-empty topics in the log.
    pub fn topics_non_empty(&self) -> Vec<LogTopic> {
        self.log.topics_non_empty()
    }

    /// Serializes itself to JSON-RPC log format.
    pub fn to_json_rpc_log(self) -> JsonValue {
        let alloy_log: AlloyLog = self.into();
        to_json_value(alloy_log)
    }

    pub fn mine_log(
        mined_log: Log,
        block_number: BlockNumber,
        block_hash: Hash,
        tx: &TransactionExecution,
        log_index: Index,
        transaction_index: Index,
    ) -> Self {
        LogMined {
            log: mined_log,
            transaction_hash: tx.input.hash,
            transaction_index,
            log_index,
            block_number,
            block_hash,
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl TryFrom<AlloyLog> for LogMined {
    type Error = anyhow::Error;
    fn try_from(value: AlloyLog) -> Result<Self, Self::Error> {
        let transaction_hash = value
            .transaction_hash
            .ok_or_else(|| anyhow::anyhow!("log must have transaction_hash"))
            .map(|bytes| Hash::from(bytes.0))?;
        let transaction_index = Index::from(value.transaction_index.ok_or_else(|| anyhow::anyhow!("log must have transaction_index"))?);
        let log_index = Index::from(value.log_index.ok_or_else(|| anyhow::anyhow!("log must have log_index"))?);
        let block_number = BlockNumber::from(value.block_number.ok_or_else(|| anyhow::anyhow!("log must have block_number"))?);
        let block_hash = value
            .block_hash
            .ok_or_else(|| anyhow::anyhow!("log must have block_hash"))
            .map(|bytes| Hash::from(bytes.0))?;

        Ok(Self {
            transaction_hash,
            transaction_index,
            log_index,
            block_number,
            block_hash,
            log: value.into(),
        })
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<LogMined> for AlloyLog {
    fn from(value: LogMined) -> Self {
        Self {
            inner: AlloyLogPrimitive {
                address: value.log.address.into(),
                // Using new_unchecked is safe because topics_non_empty() guarantees ≤ 4 topics
                data: AlloyLogData::new_unchecked(value.topics_non_empty().into_iter().map(Into::into).collect(), value.log.data.into()),
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

impl TryFrom<LogMined> for SubscriptionMessage {
    type Error = serde_json::Error;

    fn try_from(value: LogMined) -> Result<Self, Self::Error> {
        Ok(serde_json::value::RawValue::from_string(serde_json::to_string(&Into::<AlloyLog>::into(value))?)?.into())
    }
}
