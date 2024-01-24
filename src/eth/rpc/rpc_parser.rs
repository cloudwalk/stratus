//! Helper functions for parsing RPC requests and responses.

use std::fmt::Debug;
use std::fmt::Display;

use anyhow::anyhow;
use jsonrpsee::types::error::INTERNAL_ERROR_CODE;
use jsonrpsee::types::error::INTERNAL_ERROR_MSG;
use jsonrpsee::types::error::PARSE_ERROR_CODE;
use jsonrpsee::types::error::PARSE_ERROR_MSG;
use jsonrpsee::types::ErrorObject;
use jsonrpsee::types::ErrorObjectOwned;
use jsonrpsee::types::ParamsSequence;
use rlp::Decodable;

use crate::gen_newtype_from;

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
pub fn rpc_parsing_error<S: serde::Serialize>(message: S) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(PARSE_ERROR_CODE, PARSE_ERROR_MSG, Some(message))
}

/// Helper type so that we can convert anyhow::Error to ErrorObjectOwned
#[derive(Debug)]
pub struct RpcError(anyhow::Error);

impl std::error::Error for RpcError {}

impl Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

gen_newtype_from!(self = RpcError, other = anyhow::Error);

impl From<RpcError> for ErrorObjectOwned {
    fn from(value: RpcError) -> Self {
        tracing::debug!("{:?}", value.0);
        match value.0.downcast::<ErrorObject>() {
            Ok(err) => err,
            Err(err) => ErrorObjectOwned::owned(INTERNAL_ERROR_CODE, INTERNAL_ERROR_MSG, Some(err.to_string())),
        }
    }
}
