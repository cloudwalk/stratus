//! Helper functions for parsing RPC requests and responses.

use jsonrpsee::types::error::INTERNAL_ERROR_CODE;
use jsonrpsee::types::error::INTERNAL_ERROR_MSG;
use jsonrpsee::types::error::INVALID_PARAMS_CODE;
use jsonrpsee::types::error::INVALID_PARAMS_MSG;
use jsonrpsee::types::ErrorObjectOwned;
use jsonrpsee::types::ParamsSequence;
use jsonrpsee::Extensions;
use rlp::Decodable;
use tracing::Span;

use crate::eth::rpc::rpc_client_app::RpcClientApp;
use crate::eth::rpc::RpcError;

/// Extensions for jsonrpsee Extensions.
pub trait RpcExtensionsExt {
    /// Returns the client performing the JSON-RPC request.
    fn rpc_client(&self) -> RpcClientApp;

    /// Enters RpcMiddleware request span if present.
    fn enter_middleware_span(&self) -> Option<tracing::span::Entered<'_>>;
}

impl RpcExtensionsExt for Extensions {
    fn rpc_client(&self) -> RpcClientApp {
        self.get::<RpcClientApp>().cloned().unwrap_or_default()
    }

    fn enter_middleware_span(&self) -> Option<tracing::span::Entered<'_>> {
        self.get::<Span>().map(|s| s.enter())
    }
}

/// Extracts the next RPC parameter. Fails if parameter not present.
pub fn next_rpc_param<'a, T: serde::Deserialize<'a>>(mut params: ParamsSequence<'a>) -> Result<(ParamsSequence, T), RpcError> {
    match params.optional_next::<T>() {
        Ok(Some(value)) => Ok((params, value)),
        Ok(None) => Err(RpcError::ParameterMissing(std::any::type_name::<T>())),
        Err(e) => Err(RpcError::ParameterInvalid(
            std::any::type_name::<T>(),
            e.data().map(|x| x.to_string()).unwrap_or_default(),
        )),
    }
}

/// Extract the next RPC parameter. Assumes default value if not present.
pub fn next_rpc_param_or_default<'a, T: serde::Deserialize<'a> + Default>(mut params: ParamsSequence<'a>) -> Result<(ParamsSequence, T), RpcError> {
    match params.optional_next::<T>() {
        Ok(Some(value)) => Ok((params, value)),
        Ok(None) => Ok((params, T::default())),
        Err(e) => Err(RpcError::ParameterInvalid(
            std::any::type_name::<T>(),
            e.data().map(|x| x.to_string()).unwrap_or_default(),
        )),
    }
}

/// Decode an RPC parameter encoded in RLP.
pub fn parse_rpc_rlp<T: Decodable>(value: &[u8]) -> Result<T, RpcError> {
    match rlp::decode::<T>(value) {
        Ok(trx) => Ok(trx),
        Err(e) => Err(RpcError::TransactionInvalid(e)),
    }
}

/// Creates an RPC parsing error response.
pub fn rpc_params_error<S: serde::Serialize>(message: S) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(INVALID_PARAMS_CODE, INVALID_PARAMS_MSG, Some(message))
}

/// Creates an RPC internal error response.
pub fn rpc_internal_error<S: serde::Serialize>(message: S) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(INTERNAL_ERROR_CODE, INTERNAL_ERROR_MSG, Some(message))
}
