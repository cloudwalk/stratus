use jsonrpsee::types::error::CALL_EXECUTION_FAILED_CODE;
use jsonrpsee::types::error::INTERNAL_ERROR_CODE;
use jsonrpsee::types::error::INVALID_PARAMS_CODE;
use jsonrpsee::types::error::INVALID_REQUEST_CODE;
use jsonrpsee::types::error::SERVER_IS_BUSY_CODE;
use jsonrpsee::types::ErrorObject;
use jsonrpsee::types::ErrorObjectOwned;

use crate::eth::primitives::Bytes;
use crate::ext::JsonValue;

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
    SubscriptionUnknown { event: String },

    #[error("Stratus is not ready to start servicing requests.")]
    StratusNotReady,

    #[error("Stratus is shutting down.")]
    StratusShutdown,

    #[error("Failed to decode transaction RLP data.")]
    TransactionInvalidRlp { decode_error: String },

    #[error("Transaction reverted during execution.")]
    TransactionReverted { output: Bytes },

    /// Deprecated. Generic error executing RPC method.
    #[error("Unexpected error")]
    Generic(anyhow::Error),

    /// Deprecated. Custom RPC error response.
    #[error("{0}")]
    Response(ErrorObjectOwned),
}

impl RpcError {
    /// Error code to be used in the JSON-RPC response.
    pub fn code(&self) -> i32 {
        match self {
            // -----------------------------------------------------------------
            // Request
            // -----------------------------------------------------------------
            RpcError::ClientMissing => INVALID_REQUEST_CODE,

            // -----------------------------------------------------------------
            // Params
            // -----------------------------------------------------------------
            RpcError::BlockRangeInvalid { .. } => INVALID_PARAMS_CODE,
            RpcError::ParameterInvalid { .. } => INVALID_PARAMS_CODE,
            RpcError::ParameterMissing { .. } => INVALID_PARAMS_CODE,
            RpcError::SubscriptionUnknown { .. } => INVALID_PARAMS_CODE,
            RpcError::TransactionInvalidRlp { .. } => INVALID_PARAMS_CODE,

            // -----------------------------------------------------------------
            // Execution
            // -----------------------------------------------------------------
            RpcError::TransactionReverted { .. } => CALL_EXECUTION_FAILED_CODE,

            // -----------------------------------------------------------------
            // Stratus
            // -----------------------------------------------------------------
            RpcError::StratusNotReady => SERVER_IS_BUSY_CODE,
            RpcError::StratusShutdown => SERVER_IS_BUSY_CODE,

            // -----------------------------------------------------------------
            // Deprecated
            // -----------------------------------------------------------------
            RpcError::Generic(_) => INTERNAL_ERROR_CODE,
            RpcError::Response(resp) => resp.code(),
        }
    }

    /// Error message to be used in the JSON-RPC response.
    pub fn message(&self) -> String {
        match self {
            RpcError::Response(resp) => resp.message().to_string(),
            e => e.to_string(),
        }
    }

    /// Error additional data to be used in the JSON-RPC response.
    pub fn data(&self) -> JsonValue {
        match self {
            RpcError::ParameterInvalid { decode_error, .. } => JsonValue::String(decode_error.to_string()),
            RpcError::TransactionInvalidRlp { decode_error } => JsonValue::String(decode_error.to_string()),
            RpcError::TransactionReverted { output } => JsonValue::String(const_hex::encode_prefixed(output)),

            // -----------------------------------------------------------------
            // Deprecated
            // -----------------------------------------------------------------
            RpcError::Generic(error) => JsonValue::String(error.to_string()),
            RpcError::Response(resp) => JsonValue::String(resp.data().map(|v| v.to_string()).unwrap_or_default()),

            // -----------------------------------------------------------------
            // No data
            // -----------------------------------------------------------------
            _ => JsonValue::Null,
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

// TODO: remove
impl From<anyhow::Error> for RpcError {
    fn from(value: anyhow::Error) -> Self {
        match value.downcast::<ErrorObject>() {
            Ok(err) => RpcError::Response(err),
            Err(err) => RpcError::Generic(err),
        }
    }
}

// TODO: remove
impl From<ErrorObjectOwned> for RpcError {
    fn from(value: ErrorObjectOwned) -> Self {
        RpcError::Response(value)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<RpcError> for ErrorObjectOwned {
    fn from(value: RpcError) -> Self {
        // convert
        let response = match value {
            RpcError::Response(resp) => resp,
            ref e => Self::owned(e.code(), e.message(), Some(e.data())),
        };

        // log
        if [INVALID_REQUEST_CODE, INVALID_PARAMS_CODE].contains(&response.code()) {
            tracing::warn!(?response, "invalid client request");
        }
        if response.code() == INTERNAL_ERROR_CODE {
            tracing::error!(?response, "server error handling request");
        }

        response
    }
}
