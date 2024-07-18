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
    // Request
    ClientMissing,

    // Params
    BlockFilterInvalid { filter: BlockFilter },
    BlockRangeInvalid { actual: u64, max: u64 },
    ParameterMissing { rust_type: &'static str },
    ParameterInvalid { rust_type: &'static str, decode_error: String },
    SubscriptionInvalid { event: String },
    SubscriptionLimit { max_limit: String },
    TransactionInvalidRlp { decode_error: String },

    // Execution
    TransactionDisabled,
    TransactionReverted { output: Bytes },
    TransactionForwardFailed,
    TransactionConflict(Box<ExecutionConflicts>),

    // EVM
    // TODO: remove this catch-all and identify specific errors
    Evm(EVMError<anyhow::Error>),

    // Storage
    BlockConflict { number: BlockNumber },
    MinedNumberConflict { new: BlockNumber, mined: BlockNumber },
    PendingNumberConflict { new: BlockNumber, pending: BlockNumber },

    // Unexpected
    UnexpectedChannelRead { name: &'static str },
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
            Self::ClientMissing => INVALID_REQUEST_CODE,

            // RPC Params
            Self::BlockFilterInvalid { .. } => INVALID_PARAMS_CODE,
            Self::BlockRangeInvalid { .. } => INVALID_PARAMS_CODE,
            Self::ParameterInvalid { .. } => INVALID_PARAMS_CODE,
            Self::ParameterMissing { .. } => INVALID_PARAMS_CODE,
            Self::SubscriptionInvalid { .. } => INVALID_PARAMS_CODE,
            Self::SubscriptionLimit { .. } => TOO_MANY_SUBSCRIPTIONS_CODE,
            Self::TransactionInvalidRlp { .. } => INVALID_PARAMS_CODE,

            // Execution
            Self::TransactionDisabled => INTERNAL_ERROR_CODE,
            Self::TransactionForwardFailed => INTERNAL_ERROR_CODE,
            Self::TransactionReverted { .. } => CALL_EXECUTION_FAILED_CODE,
            Self::TransactionConflict(_) => INTERNAL_ERROR_CODE,

            // EVM
            Self::Evm(_) => INTERNAL_ERROR_CODE,

            // Storage
            Self::BlockConflict { .. } => INTERNAL_ERROR_CODE,
            Self::MinedNumberConflict { .. } => INTERNAL_ERROR_CODE,
            Self::PendingNumberConflict { .. } => INTERNAL_ERROR_CODE,

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
            Self::ClientMissing => "Denied because client did not identify itself.".to_owned(),

            // RPC Params
            Self::BlockFilterInvalid { .. } => "Block filter does not point to a valid block.".into(),
            Self::BlockRangeInvalid { actual, max } => format!("Denied because will fetch data from {actual} blocks, but the max allowed is {max}."),
            Self::ParameterMissing { rust_type } => format!("Expected {rust_type} parameter, but received nothing."),
            Self::ParameterInvalid { rust_type, .. } => format!("Failed to decode {rust_type} parameter."),
            Self::SubscriptionInvalid { event } => format!("Invalid subscription event: {event}"),
            Self::SubscriptionLimit { max_limit } => format!("Client has reached the maximum subscription limit of {max_limit}."),
            Self::TransactionInvalidRlp { .. } => "Failed to decode transaction RLP data.".into(),

            // Execution
            Self::TransactionDisabled => "Transaction processing is temporarily disabled.".into(),
            Self::TransactionReverted { .. } => "Transaction reverted during execution.".into(),
            Self::TransactionForwardFailed => "Failed to forward transaction to leader node.".into(),
            Self::TransactionConflict(conflicts) => format!("Execution conflicts: {conflicts:?}"),

            // EVM
            Self::Evm(_) => "Failed to executed transaction in EVM.".into(),

            // Storage
            Self::BlockConflict { number } => format!("Block conflict: {number} already exists in the permanent storage."),
            Self::MinedNumberConflict { new, mined } => format!("Mined number conflict between new block number ({new}) and mined block number ({mined})."),
            Self::PendingNumberConflict { new, pending } =>
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
            Self::BlockFilterInvalid { filter } => Some(to_json_string(filter)),
            Self::ParameterInvalid { decode_error, .. } => Some(decode_error.to_string()),

            // Transaction
            Self::TransactionInvalidRlp { decode_error } => Some(decode_error.to_string()),
            Self::TransactionReverted { output } => Some(const_hex::encode_prefixed(output)),

            // EVM
            Self::Evm(e) => Some(e.to_string()),

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
