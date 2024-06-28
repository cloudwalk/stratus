//! Helper functions for parsing RPC requests and responses.

use anyhow::anyhow;
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

/// Extensions for jsonrpsee Extensions.
pub trait RpcExtensionsExt {
    /// Returns the client performing the JSON-RPC request.
    fn rpc_client(&self) -> RpcClientApp;

    /// Enters the RpcMiddleware request span if present.
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
pub fn next_rpc_param<'a, T: serde::Deserialize<'a>>(mut params: ParamsSequence<'a>) -> anyhow::Result<(ParamsSequence, T)> {
    match params.next::<T>() {
        Ok(value) => Ok((params, value)),
        Err(e) => {
            tracing::warn!(reason = ?e, kind = std::any::type_name::<T>(), "failed to parse rpc param");
            Err(e.into())
        }
    }
}

/// Extract the next RPC parameter. Assumes default value if not present.
pub fn next_rpc_param_or_default<'a, T: serde::Deserialize<'a> + Default>(mut params: ParamsSequence<'a>) -> anyhow::Result<(ParamsSequence, T)> {
    match params.optional_next::<T>() {
        Ok(Some(value)) => Ok((params, value)),
        Ok(None) => Ok((params, T::default())),
        Err(e) => {
            tracing::warn!(reason = ?e, kind = std::any::type_name::<T>(), "failed to parse rpc param");
            Err(e.into())
        }
    }
}

/// Decode an RPC parameter encoded in RLP.
pub fn parse_rpc_rlp<T: Decodable>(value: &[u8]) -> anyhow::Result<T> {
    match rlp::decode::<T>(value) {
        Ok(trx) => Ok(trx),
        Err(e) => {
            tracing::warn!(reason = ?e, "failed to decode rlp data");
            Err(anyhow!("Parse error {:?}", Some(value)))
        }
    }
}

/// Creates an RPC parsing error response.
pub fn rpc_invalid_params_error<S: serde::Serialize>(message: S) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(INVALID_PARAMS_CODE, INVALID_PARAMS_MSG, Some(message))
}

/// Creates an RPC internal error response.
pub fn rpc_internal_error<S: serde::Serialize>(message: S) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(INTERNAL_ERROR_CODE, INTERNAL_ERROR_MSG, Some(message))
}
