use jsonrpsee::types::error::INTERNAL_ERROR_CODE;
use jsonrpsee::types::error::INTERNAL_ERROR_MSG;
use jsonrpsee::types::error::INVALID_REQUEST_CODE;
use jsonrpsee::types::error::INVALID_REQUEST_MSG;
use jsonrpsee::types::ErrorObject;
use jsonrpsee::types::ErrorObjectOwned;

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("Denied because will fetch data from {actual} blocks, but the max allowed is {max}.")]
    BlockRangeInvalid { actual: u64, max: u64 },

    #[error("Denied because client did not identify itself.")]
    ClientMissing,

    #[error("Expected {rust_type} parameter, but received nothing.")]
    ParameterMissing { rust_type: &'static str },

    #[error("Failed to decode {rust_type} parameter: {decode_error}.")]
    ParameterInvalid { rust_type: &'static str, decode_error: String },

    #[error("Unknown subscription event: {event}")]
    SubscriptionUnknown { event: String },

    #[error("Failed to decode transaction data from RLP payload: {decode_error}.")]
    TransactionInvalid { decode_error: String },

    #[error("Stratus is not ready to start servicing requests.")]
    StratusNotReady,

    #[error("Stratus is shutting down.")]
    StratusShutdown,

    /// Deprecated. Generic error executing RPC method.
    #[error("RPC error: {0}")]
    Generic(anyhow::Error),

    /// Deprecated. Custom RPC error response.
    #[error("{0}")]
    Response(ErrorObjectOwned),
}

impl RpcError {
    /// Decides the error code and message to be used according to the error type.
    pub fn response_code(&self) -> (i32, &str) {
        match self {
            RpcError::BlockRangeInvalid { .. } => (INVALID_REQUEST_CODE, INVALID_REQUEST_MSG),
            RpcError::ClientMissing => (INVALID_REQUEST_CODE, INVALID_REQUEST_MSG),
            RpcError::ParameterInvalid { .. } => (INVALID_REQUEST_CODE, INVALID_REQUEST_MSG),
            RpcError::ParameterMissing { .. } => (INVALID_REQUEST_CODE, INVALID_REQUEST_MSG),
            RpcError::SubscriptionUnknown { .. } => (INVALID_REQUEST_CODE, INVALID_REQUEST_MSG),
            RpcError::TransactionInvalid { .. } => (INVALID_REQUEST_CODE, INVALID_REQUEST_MSG),
            // TODO: use another code for status endpoints instead of internal error
            RpcError::StratusNotReady => (INTERNAL_ERROR_CODE, INTERNAL_ERROR_MSG),
            RpcError::StratusShutdown => (INTERNAL_ERROR_CODE, INTERNAL_ERROR_MSG),
            // TODO: remove these variants
            RpcError::Generic(_) => (INTERNAL_ERROR_CODE, INTERNAL_ERROR_MSG),
            RpcError::Response(r) => (r.code(), r.message()),
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
        let (code, message) = value.response_code();
        let reason = value.to_string();
        if code == INVALID_REQUEST_CODE {
            tracing::warn!(%reason, "invalid client request");
        }
        if code == INTERNAL_ERROR_CODE {
            tracing::error!(%reason, "internal error handling request");
        }
        Self::owned(code, message, Some(reason))
    }
}
