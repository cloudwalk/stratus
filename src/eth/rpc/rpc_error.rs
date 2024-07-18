use std::error::Error;

use jsonrpsee::types::error::CALL_EXECUTION_FAILED_CODE;
use jsonrpsee::types::error::INTERNAL_ERROR_CODE;
use jsonrpsee::types::error::INVALID_PARAMS_CODE;
use jsonrpsee::types::error::INVALID_REQUEST_CODE;
use jsonrpsee::types::error::SERVER_IS_BUSY_CODE;
use jsonrpsee::types::error::TOO_MANY_SUBSCRIPTIONS_CODE;
use jsonrpsee::types::ErrorObjectOwned;

use crate::eth::primitives::Bytes;
use crate::infra::metrics::MetricLabelValue;

#[derive(Debug, strum::Display, strum::EnumMessage)]
#[strum(serialize_all = "kebab-case")]
pub enum RpcError {
    // Request
    ClientMissing,

    // Params
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

    // Unexpected
    Unexpected(anyhow::Error),

    // Stratus
    StratusNotReady,
    StratusShutdown,
}

impl RpcError {
    /// Error code to be used in the JSON-RPC response.
    pub fn code(&self) -> i32 {
        match self {
            // Request
            Self::ClientMissing => INVALID_REQUEST_CODE,

            // Params
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

            // Unexpected
            Self::Unexpected(_) => INTERNAL_ERROR_CODE,

            // Stratus
            Self::StratusNotReady => SERVER_IS_BUSY_CODE,
            Self::StratusShutdown => SERVER_IS_BUSY_CODE,
        }
    }

    /// Error message to be used in the JSON-RPC response.
    pub fn message(&self) -> String {
        match self {
            // Request
            Self::ClientMissing => "Denied because client did not identify itself.".to_owned(),

            // Params
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

            // Unexpected
            Self::Unexpected(_) => "Unexpected error.".into(),

            // Stratus
            Self::StratusNotReady => "Stratus is not ready to start servicing requests.".into(),
            Self::StratusShutdown => "Stratus is shutting down.".into(),
        }
    }

    /// Error additional data to be used in the JSON-RPC response.
    pub fn data(&self) -> Option<String> {
        match self {
            Self::ParameterInvalid { decode_error, .. } => Some(decode_error.to_string()),
            Self::TransactionInvalidRlp { decode_error } => Some(decode_error.to_string()),
            Self::TransactionReverted { output } => Some(const_hex::encode_prefixed(output)),
            Self::Unexpected(error) => Some(error.to_string()),
            _ => None,
        }
    }
}

impl Error for RpcError {}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl From<anyhow::Error> for RpcError {
    fn from(value: anyhow::Error) -> Self {
        Self::Unexpected(value)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<RpcError> for ErrorObjectOwned {
    fn from(value: RpcError) -> Self {
        Self::owned(value.code(), value.message(), value.data())
    }
}

impl From<&RpcError> for MetricLabelValue {
    fn from(value: &RpcError) -> Self {
        Self::Some(value.to_string())
    }
}
