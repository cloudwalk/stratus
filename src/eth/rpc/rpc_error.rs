use jsonrpsee::types::error::CALL_EXECUTION_FAILED_CODE;
use jsonrpsee::types::error::INTERNAL_ERROR_CODE;
use jsonrpsee::types::error::INVALID_PARAMS_CODE;
use jsonrpsee::types::error::INVALID_REQUEST_CODE;
use jsonrpsee::types::error::SERVER_IS_BUSY_CODE;
use jsonrpsee::types::ErrorObjectOwned;

use crate::eth::primitives::Bytes;

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("Denied because will fetch data from {actual} blocks, but the max allowed is {max}.")]
    BlockRangeInvalid { actual: u64, max: u64 },

    #[error("Denied because client did not identify itself.")]
    ClientMissing,

    #[error("Expected {rust_type} parameter, but received nothing.")]
    ParameterMissing { rust_type: &'static str },

    #[error("Failed to decode {rust_type} parameter.")]
    ParameterInvalid { rust_type: &'static str, decode_error: String },

    #[error("Unknown subscription event: {event}")]
    SubscriptionInvalid { event: String },

    #[error("Stratus is not ready to start servicing requests.")]
    StratusNotReady,

    #[error("Stratus is shutting down.")]
    StratusShutdown,

    #[error("Failed to decode transaction RLP data.")]
    TransactionInvalidRlp { decode_error: String },

    #[error("Transaction reverted during execution.")]
    TransactionReverted { output: Bytes },

    #[error("Failed to forward transaction to leader node.")]
    TransactionForwardFailed,

    #[error("Unexpected error.")]
    Unexpected(anyhow::Error),
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
            Self::TransactionInvalidRlp { .. } => INVALID_PARAMS_CODE,

            // Execution
            Self::TransactionForwardFailed => INTERNAL_ERROR_CODE,
            Self::TransactionReverted { .. } => CALL_EXECUTION_FAILED_CODE,

            // Stratus
            Self::StratusNotReady => SERVER_IS_BUSY_CODE,
            Self::StratusShutdown => SERVER_IS_BUSY_CODE,

            // Unexpected
            Self::Unexpected(_) => INTERNAL_ERROR_CODE,
        }
    }

    /// Error message to be used in the JSON-RPC response.
    pub fn message(&self) -> String {
        self.to_string()
    }

    /// Error additional data to be used in the JSON-RPC response.
    pub fn data(&self) -> Option<String> {
        match self {
            RpcError::ParameterInvalid { decode_error, .. } => Some(decode_error.to_string()),
            RpcError::TransactionInvalidRlp { decode_error } => Some(decode_error.to_string()),
            RpcError::TransactionReverted { output } => Some(const_hex::encode_prefixed(output)),
            RpcError::Unexpected(error) => Some(error.to_string()),
            _ => None,
        }
    }
}

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
