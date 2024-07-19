use std::error::Error;

use jsonrpsee::types::error::CALL_EXECUTION_FAILED_CODE;
use jsonrpsee::types::error::INTERNAL_ERROR_CODE;
use jsonrpsee::types::error::INVALID_PARAMS_CODE;
use jsonrpsee::types::error::INVALID_REQUEST_CODE;
use jsonrpsee::types::error::SERVER_IS_BUSY_CODE;
use jsonrpsee::types::error::TOO_MANY_SUBSCRIPTIONS_CODE;
use jsonrpsee::types::ErrorObjectOwned;
use revm::primitives::EVMError;

use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ExecutionConflicts;
use crate::ext::to_json_string;
use crate::infra::metrics::MetricLabelValue;

#[derive(Debug, strum::Display, strum::EnumMessage)]
pub enum StratusError {
    // RPC
    RpcBlockFilterInvalid { filter: BlockFilter },
    RpcBlockRangeInvalid { actual: u64, max: u64 },
    RpcClientMissing,
    RpcParameterInvalid { rust_type: &'static str, decode_error: String },
    RpcParameterMissing { rust_type: &'static str },
    RpcSubscriptionInvalid { event: String },
    RpcSubscriptionLimit { max: u32 },
    RpcTransactionDisabled,
    RpcTransactionInvalid { decode_error: String },

    // Transaction
    TransactionConflict(Box<ExecutionConflicts>),
    TransactionFailed(EVMError<anyhow::Error>), // split this in multiple errors
    TransactionForwardToLeaderFailed,
    TransactionReverted { output: Bytes },

    // Storage
    StorageBlockConflict { number: BlockNumber },
    StorageMinedNumberConflict { new: BlockNumber, mined: BlockNumber },
    StoragePendingNumberConflict { new: BlockNumber, pending: BlockNumber },

    // Unexpected
    UnexpectedChannelRead { channel: &'static str },
    Unexpected(anyhow::Error),

    // Stratus state
    StratusNotReady,
    StratusShutdown,
}

impl StratusError {
    /// Error code to be used in JSON-RPC response.
    pub fn rpc_code(&self) -> i32 {
        match self {
            // RPC Request
            Self::RpcClientMissing => INVALID_REQUEST_CODE,

            // RPC Params
            Self::RpcBlockFilterInvalid { .. } => INVALID_PARAMS_CODE,
            Self::RpcBlockRangeInvalid { .. } => INVALID_PARAMS_CODE,
            Self::RpcParameterInvalid { .. } => INVALID_PARAMS_CODE,
            Self::RpcParameterMissing { .. } => INVALID_PARAMS_CODE,
            Self::RpcSubscriptionInvalid { .. } => INVALID_PARAMS_CODE,
            Self::RpcSubscriptionLimit { .. } => TOO_MANY_SUBSCRIPTIONS_CODE,
            Self::RpcTransactionInvalid { .. } => INVALID_PARAMS_CODE,

            // Execution
            Self::RpcTransactionDisabled => INTERNAL_ERROR_CODE,
            Self::TransactionForwardToLeaderFailed => INTERNAL_ERROR_CODE,
            Self::TransactionReverted { .. } => CALL_EXECUTION_FAILED_CODE,
            Self::TransactionConflict(_) => INTERNAL_ERROR_CODE,

            // EVM
            Self::TransactionFailed(_) => INTERNAL_ERROR_CODE,

            // Storage
            Self::StorageBlockConflict { .. } => INTERNAL_ERROR_CODE,
            Self::StorageMinedNumberConflict { .. } => INTERNAL_ERROR_CODE,
            Self::StoragePendingNumberConflict { .. } => INTERNAL_ERROR_CODE,

            // Unexpected
            Self::Unexpected(_) => INTERNAL_ERROR_CODE,
            Self::UnexpectedChannelRead { .. } => INTERNAL_ERROR_CODE,

            // Stratus state
            Self::StratusNotReady => SERVER_IS_BUSY_CODE,
            Self::StratusShutdown => SERVER_IS_BUSY_CODE,
        }
    }

    /// Error message to be used in JSON-RPC response.
    pub fn rpc_message(&self) -> String {
        match self {
            // RPC Request
            Self::RpcClientMissing => "Denied because client did not identify itself.".to_owned(),

            // RPC Params
            Self::RpcBlockFilterInvalid { .. } => "Block filter does not point to a valid block.".into(),
            Self::RpcBlockRangeInvalid { actual, max } => format!("Denied because will fetch data from {actual} blocks, but the max allowed is {max}."),
            Self::RpcParameterMissing { rust_type } => format!("Expected {rust_type} parameter, but received nothing."),
            Self::RpcParameterInvalid { rust_type, .. } => format!("Failed to decode {rust_type} parameter."),
            Self::RpcSubscriptionInvalid { event } => format!("Invalid subscription event: {event}"),
            Self::RpcSubscriptionLimit { max: max_limit } => format!("Client has reached the maximum subscription limit of {max_limit}."),
            Self::RpcTransactionInvalid { .. } => "Failed to decode transaction RLP data.".into(),

            // Execution
            Self::RpcTransactionDisabled => "Transaction processing is temporarily disabled.".into(),
            Self::TransactionReverted { .. } => "Transaction reverted during execution.".into(),
            Self::TransactionForwardToLeaderFailed => "Failed to forward transaction to leader node.".into(),
            Self::TransactionConflict(conflicts) => format!("Execution conflicts: {conflicts:?}"),

            // EVM
            Self::TransactionFailed(_) => "Failed to executed transaction in EVM.".into(),

            // Storage
            Self::StorageBlockConflict { number } => format!("Block conflict: {number} already exists in the permanent storage."),
            Self::StorageMinedNumberConflict { new, mined } =>
                format!("Mined number conflict between new block number ({new}) and mined block number ({mined})."),
            Self::StoragePendingNumberConflict { new, pending } =>
                format!("Pending number conflict between new block number ({new}) and pending block number ({pending})."),

            // Unexpected
            Self::Unexpected(_) => "Unexpected error.".into(),
            Self::UnexpectedChannelRead { .. } => "Unexpected channel error.".into(),

            // Stratus state
            Self::StratusNotReady => "Stratus is not ready to start servicing requests.".into(),
            Self::StratusShutdown => "Stratus is shutting down.".into(),
        }
    }

    /// Error additional data to be used in JSON-RPC response.
    pub fn rpc_data(&self) -> Option<String> {
        match self {
            // RPC
            Self::RpcBlockFilterInvalid { filter } => Some(to_json_string(filter)),
            Self::RpcParameterInvalid { decode_error, .. } => Some(decode_error.to_string()),

            // Transaction
            Self::RpcTransactionInvalid { decode_error } => Some(decode_error.to_string()),
            Self::TransactionFailed(e) => Some(e.to_string()),
            Self::TransactionReverted { output } => Some(const_hex::encode_prefixed(output)),

            // Unexpected
            Self::Unexpected(error) => Some(error.to_string()),
            _ => None,
        }
    }
}

impl Error for StratusError {}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl From<anyhow::Error> for StratusError {
    fn from(value: anyhow::Error) -> Self {
        Self::Unexpected(value)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<StratusError> for ErrorObjectOwned {
    fn from(value: StratusError) -> Self {
        Self::owned(value.rpc_code(), value.rpc_message(), value.rpc_data())
    }
}

impl From<&StratusError> for MetricLabelValue {
    fn from(value: &StratusError) -> Self {
        Self::Some(value.to_string())
    }
}
