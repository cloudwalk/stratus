use jsonrpsee::types::error::INTERNAL_ERROR_CODE;
use jsonrpsee::types::error::INTERNAL_ERROR_MSG;
use jsonrpsee::types::ErrorObject;
use jsonrpsee::types::ErrorObjectOwned;

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    /// Generic error executing RPC method.
    #[error("RPC error: {0}")]
    Generic(anyhow::Error),

    /// Custom RPC error response.
    #[error("{0}")]
    Response(ErrorObjectOwned),
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl From<anyhow::Error> for RpcError {
    fn from(value: anyhow::Error) -> Self {
        tracing::error!(reason = ?value, "rpc error response");
        match value.downcast::<ErrorObject>() {
            Ok(err) => RpcError::Response(err),
            Err(err) => RpcError::Generic(err),
        }
    }
}

impl From<ErrorObjectOwned> for RpcError {
    fn from(value: ErrorObjectOwned) -> Self {
        tracing::warn!(reason = ?value, "rpc warning response");
        RpcError::Response(value)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<RpcError> for ErrorObjectOwned {
    fn from(value: RpcError) -> Self {
        match value {
            RpcError::Response(err) => err,
            RpcError::Generic(err) => ErrorObjectOwned::owned(INTERNAL_ERROR_CODE, INTERNAL_ERROR_MSG, Some(err.to_string())),
        }
    }
}
