use jsonrpsee::types::error::INTERNAL_ERROR_CODE;
use jsonrpsee::types::error::INTERNAL_ERROR_MSG;
use jsonrpsee::types::error::INVALID_REQUEST_CODE;
use jsonrpsee::types::error::INVALID_REQUEST_MSG;
use jsonrpsee::types::ErrorObject;
use jsonrpsee::types::ErrorObjectOwned;
use rlp::DecoderError;

type RustType = &'static str;

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("Client did not identify itself.")]
    ClientMissing,

    #[error("Expected {0} parameter, but received nothing.")]
    ParameterMissing(RustType),

    #[error("Failed to decode {0} parameter: {1}.")]
    ParameterInvalid(RustType, String),

    #[error("Failed to decode transaction data from RLP payload: {0}.")]
    TransactionInvalid(DecoderError),

    /// Deprecated. Generic error executing RPC method.
    #[error("RPC error: {0}")]
    Generic(anyhow::Error),

    /// Deprecated. Custom RPC error response.
    #[error("{0}")]
    Response(ErrorObjectOwned),
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl From<anyhow::Error> for RpcError {
    fn from(value: anyhow::Error) -> Self {
        match value.downcast::<ErrorObject>() {
            Ok(err) => RpcError::Response(err),
            Err(err) => RpcError::Generic(err),
        }
    }
}

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
        let data = value.to_string();

        match value {
            RpcError::Response(err) => err,
            RpcError::Generic(err) => Self::owned(INTERNAL_ERROR_CODE, INTERNAL_ERROR_MSG, Some(err.to_string())),
            //
            RpcError::ClientMissing => Self::owned(INVALID_REQUEST_CODE, INVALID_REQUEST_MSG, Some(data)),
            RpcError::ParameterMissing(_) => Self::owned(INVALID_REQUEST_CODE, INVALID_REQUEST_MSG, Some(data)),
            RpcError::ParameterInvalid(_, _) => Self::owned(INVALID_REQUEST_CODE, INVALID_REQUEST_MSG, Some(data)),
            RpcError::TransactionInvalid(_) => Self::owned(INVALID_REQUEST_CODE, INVALID_REQUEST_MSG, Some(data)),
        }
    }
}
