//! Helper functions for parsing RPC requests and responses.

use alloy_rlp::Decodable;
use jsonrpsee::types::ParamsSequence;
use jsonrpsee::Extensions;
use tracing::Span;

use super::rpc_http_middleware::Authentication;
use crate::eth::primitives::RpcError;
use crate::eth::rpc::rpc_client_app::RpcClientApp;
use crate::ext::type_basename;
use crate::infra::tracing::EnteredWrap;

/// Extensions for jsonrpsee Extensions.
pub trait RpcExtensionsExt {
    /// Returns the client performing the JSON-RPC request.
    fn rpc_client(&self) -> &RpcClientApp;

    /// Returns current Authentication.
    fn authentication(&self) -> &Authentication;

    /// Returns wheather admin authentication suceeded.
    fn is_admin(&self) -> bool;

    /// Enters RpcMiddleware request span if present.
    fn enter_middleware_span(&self) -> Option<EnteredWrap<'_>>;
}

impl RpcExtensionsExt for Extensions {
    fn rpc_client(&self) -> &RpcClientApp {
        self.get::<RpcClientApp>().unwrap_or(&RpcClientApp::Unknown)
    }

    fn authentication(&self) -> &Authentication {
        self.get::<Authentication>().unwrap_or(&Authentication::None)
    }

    fn is_admin(&self) -> bool {
        matches!(self.authentication(), Authentication::Admin)
    }

    fn enter_middleware_span(&self) -> Option<EnteredWrap<'_>> {
        self.get::<Span>().map(|s| s.enter()).map(EnteredWrap::new)
    }
}

/// Extracts the next RPC parameter. Fails if parameter not present.
pub fn next_rpc_param<'a, T>(mut params: ParamsSequence<'a>) -> Result<(ParamsSequence<'a>, T), RpcError>
where
    T: serde::Deserialize<'a>,
{
    match params.optional_next::<T>() {
        Ok(Some(value)) => Ok((params, value)),
        Ok(None) => Err(RpcError::ParameterMissing {
            rust_type: type_basename::<T>(),
        }),
        Err(e) => Err(RpcError::ParameterInvalid {
            rust_type: type_basename::<T>(),
            decode_error: e.data().map(|x| x.to_string()).unwrap_or_default(),
        }),
    }
}

/// Extract the next RPC parameter. Assumes default value if not present.
pub fn next_rpc_param_or_default<'a, T>(params: ParamsSequence<'a>) -> Result<(ParamsSequence<'a>, T), RpcError>
where
    T: serde::Deserialize<'a> + Default,
{
    match next_rpc_param(params) {
        Ok((params, value)) => Ok((params, value)),
        Err(RpcError::ParameterMissing { .. }) => Ok((params, T::default())),
        Err(e) => Err(e),
    }
}

/// Decode an RPC parameter encoded in RLP.
///
/// https://ethereum.org/en/developers/docs/data-structures-and-encoding/rlp
pub fn parse_rpc_rlp<T: Decodable>(value: &[u8]) -> Result<T, RpcError> {
    let mut buf = value;
    match T::decode(&mut buf) {
        Ok(trx) => Ok(trx),
        Err(e) => Err(RpcError::TransactionInvalid { decode_error: e.to_string() }),
    }
}
