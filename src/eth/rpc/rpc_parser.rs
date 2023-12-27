//! Helper functions for parsing RPC requests and responses.

use jsonrpsee::types::error::PARSE_ERROR_CODE;
use jsonrpsee::types::error::PARSE_ERROR_MSG;
use jsonrpsee::types::ErrorObjectOwned;
use jsonrpsee::types::ParamsSequence;
use rlp::Decodable;

/// Extract the next RPC parameter from the parameters sequence.
pub fn next_rpc_param<'a, T: serde::Deserialize<'a>>(mut params: ParamsSequence<'a>) -> Result<(ParamsSequence, T), ErrorObjectOwned> {
    match params.next::<T>() {
        Ok(address) => Ok((params, address)),
        Err(e) => {
            tracing::warn!(reason = ?e, kind = std::any::type_name::<T>(), "failed to parse input param");
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
            Err(ErrorObjectOwned::owned(PARSE_ERROR_CODE, PARSE_ERROR_MSG, Some(value)))
        }
    }
}
