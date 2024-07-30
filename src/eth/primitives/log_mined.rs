use display_json::DebugAsJson;
use itertools::Itertools;
use jsonrpsee::SubscriptionMessage;

use crate::alias::EthersLog;
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
    pub fn address(&self) -> &Address {
        &self.log.address
    }

    /// Returns the topics emitted in the log.
    pub fn topics(&self) -> Vec<LogTopic> {
        self.log.topics()
    }

    /// Serializes itself to JSON-RPC log format.
    pub fn to_json_rpc_log(self) -> JsonValue {
        let ethers_log: EthersLog = self.into();
        to_json_value(ethers_log)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl TryFrom<EthersLog> for LogMined {
    type Error = anyhow::Error;
    fn try_from(value: EthersLog) -> Result<Self, Self::Error> {
        Ok(Self {
            transaction_hash: value.transaction_hash.expect("log must have transaction_hash").into(),
            transaction_index: value.transaction_index.expect("log must have transaction_index").into(),
            log_index: value.log_index.expect("log must have log_index").try_into()?,
            block_number: value.block_number.expect("log must have block_number").into(),
            block_hash: value.block_hash.expect("log must have block_hash").into(),
            log: value.into(),
        })
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
            topics: value.topics().into_iter().map_into().collect_vec(),
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
