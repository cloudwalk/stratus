use stratus_macros::ErrorCode;

use crate::eth::rpc::BlockFilter;
use crate::eth::types::ErrorCode;

/// Errors that can occur while decoding a raw transaction.
#[derive(Debug, thiserror::Error)]
pub enum TransactionDecodeError {
    #[error("missing field: {0}")]
    MissingField(&'static str),

    #[error("invalid to field")]
    InvalidTo,

    #[error("failed to recover signer")]
    SignerRecovery,

    #[error("unsupported transaction type")]
    UnsupportedType,

    #[error("typed transaction has extra fields")]
    ExtraFields,

    #[error("invalid transaction type byte")]
    InvalidTypeByte,

    #[error("empty transaction bytes")]
    EmptyBytes,

    #[error("legacy transaction type is not typed")]
    LegacyNotTyped,

    #[error("{0}")]
    Custom(String),
}

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr, ErrorCode)]
#[major_error_code = 1000]
pub enum RpcError {
    #[error("block filter does not point to a valid block.")]
    #[error_code = 1]
    BlockFilterInvalid { filter: BlockFilter },

    #[error("denied because will fetch data from {actual} blocks, but the max allowed is {max:?} and min allowed is 1.")]
    #[error_code = 2]
    BlockRangeInvalid { actual: i128, max: Option<u64> },

    #[error("denied because client did not identify itself.")]
    #[error_code = 3]
    ClientMissing,

    #[error("denied because client is blocked.")]
    #[error_code = 11]
    ClientBlocked { client: String },

    #[error("failed to decode {rust_type} parameter.")]
    #[error_code = 4]
    ParameterDecodeError { rust_type: &'static str, decode_error: String },

    #[error("expected {rust_type} parameter, but received nothing.")]
    #[error_code = 5]
    ParameterMissing { rust_type: &'static str },

    #[error("invalid subscription event: {event}")]
    #[error_code = 6]
    SubscriptionInvalid { event: String },

    #[error("denied because reached maximum subscription limit of {max}.")]
    #[error_code = 7]
    SubscriptionLimit { max: u32 },

    #[error("failed to decode transaction RLP data: {decode_error}")]
    #[error_code = 8]
    TransactionInvalid { decode_error: TransactionDecodeError },

    #[error("miner mode param is invalid.")]
    #[error_code = 9]
    MinerModeParamInvalid,

    #[error("parameter is invalid")]
    #[error_code = 10]
    ParameterInvalid,
}

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr, ErrorCode)]
#[major_error_code = 8000]
pub enum MulticallError {
    #[error("failed to decode multicall ABI: {source}")]
    #[error_code = 1]
    DecodeError {
        #[from]
        source: alloy_sol_types::Error,
    },

    #[error("unsupported multicall function")]
    #[error_code = 2]
    UnsupportedMulticallFunction,
}
