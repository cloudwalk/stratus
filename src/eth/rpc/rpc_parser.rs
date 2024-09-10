//! Helper functions for parsing RPC requests and responses.

use jsonrpsee::types::ParamsSequence;
use jsonrpsee::Extensions;
use rlp::Decodable;
use tracing::Span;

use crate::eth::primitives::StratusError;
use crate::eth::rpc::rpc_client_app::RpcClientApp;
use crate::ext::type_basename;

/// Extensions for jsonrpsee Extensions.
pub trait RpcExtensionsExt {
    /// Returns the client performing the JSON-RPC request.
    fn rpc_client(&self) -> RpcClientApp;

    /// Enters RpcMiddleware request span if present.
    fn enter_middleware_span(&self) -> Option<tracing::span::Entered<'_>>;
}

impl RpcExtensionsExt for Extensions {
    fn rpc_client(&self) -> RpcClientApp {
        self.get::<RpcClientApp>().cloned().unwrap_or(RpcClientApp::Unknown)
    }

    fn enter_middleware_span(&self) -> Option<tracing::span::Entered<'_>> {
        self.get::<Span>().map(|s| s.enter())
    }
}

/// Extracts the next RPC parameter. Fails if parameter not present.
pub fn next_rpc_param<'a, T>(mut params: ParamsSequence<'a>) -> Result<(ParamsSequence, T), StratusError>
where
    T: serde::Deserialize<'a>,
{
    match params.optional_next::<T>() {
        Ok(Some(value)) => Ok((params, value)),
        Ok(None) => Err(StratusError::RpcParameterMissing {
            rust_type: type_basename::<T>(),
        }),
        Err(e) => Err(StratusError::RpcParameterInvalid {
            rust_type: type_basename::<T>(),
            decode_error: e.data().map(|x| x.to_string()).unwrap_or_default(),
        }),
    }
}

/// Extract the next RPC parameter. Assumes default value if not present.
pub fn next_rpc_param_or_default<'a, T>(params: ParamsSequence<'a>) -> Result<(ParamsSequence, T), StratusError>
where
    T: serde::Deserialize<'a> + Default,
{
    match next_rpc_param(params) {
        Ok((params, value)) => Ok((params, value)),
        Err(StratusError::RpcParameterMissing { .. }) => Ok((params, T::default())),
        Err(e) => Err(e),
    }
}

/// Decode an RPC parameter encoded in RLP.
///
/// https://ethereum.org/en/developers/docs/data-structures-and-encoding/rlp
pub fn parse_rpc_rlp<T: Decodable>(value: &[u8]) -> Result<T, StratusError> {
    match rlp::decode::<T>(value) {
        Ok(trx) => Ok(trx),
        Err(e) => Err(StratusError::RpcTransactionInvalid { decode_error: e.to_string() }),
    }
}
