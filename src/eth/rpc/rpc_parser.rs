//! Helper functions for parsing RPC requests and responses.

use jsonrpsee::types::error::INTERNAL_ERROR_CODE;
use jsonrpsee::types::error::INTERNAL_ERROR_MSG;
use jsonrpsee::types::error::PARSE_ERROR_CODE;
use jsonrpsee::types::error::PARSE_ERROR_MSG;
use jsonrpsee::types::ErrorObjectOwned;
use jsonrpsee::types::ParamsSequence;
use rlp::Decodable;

use crate::eth::EthError;

/// Extracts the next RPC parameter. Fails if parameter not present.
pub fn next_rpc_param<'a, T: serde::Deserialize<'a>>(mut params: ParamsSequence<'a>) -> Result<(ParamsSequence, T), ErrorObjectOwned> {
    match params.next::<T>() {
        Ok(value) => Ok((params, value)),
        Err(e) => {
            tracing::warn!(reason = ?e, kind = std::any::type_name::<T>(), "failed to parse rpc param");
            Err(e)
        }
    }
}

/// Extract the next RPC parameter. Assumes default value if not present.
pub fn next_rpc_param_or_default<'a, T: serde::Deserialize<'a> + Default>(mut params: ParamsSequence<'a>) -> Result<(ParamsSequence, T), ErrorObjectOwned> {
    match params.optional_next::<T>() {
        Ok(Some(value)) => Ok((params, value)),
        Ok(None) => Ok((params, T::default())),
        Err(e) => {
            tracing::warn!(reason = ?e, kind = std::any::type_name::<T>(), "failed to parse rpc param");
            Err(e)
        }
    }
}

/// Decode an RPC parameter encoded in RLP.
pub fn parse_rpc_rlp<T: Decodable>(value: &[u8]) -> Result<T, ErrorObjectOwned> {
    match rlp::decode::<T>(value) {
        Ok(trx) => Ok(trx),
        Err(e) => {
            tracing::warn!(reason = ?e, "failed to decode rlp data");
            Err(rpc_parsing_error(Some(value)))
        }
    }
}

/// Creates an RPC internal error response.
pub fn rpc_internal_error<S: serde::Serialize>(message: S) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(INTERNAL_ERROR_CODE, INTERNAL_ERROR_MSG, Some(message))
}

/// Creates an RPC parsing error response.
pub fn rpc_parsing_error<S: serde::Serialize>(message: S) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(PARSE_ERROR_CODE, PARSE_ERROR_MSG, Some(message))
}

impl From<EthError> for ErrorObjectOwned {
    fn from(error: EthError) -> Self {
        rpc_internal_error(error.to_string())
    }
}
