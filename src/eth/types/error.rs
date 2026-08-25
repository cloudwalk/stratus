use futures::future::BoxFuture;
use jsonrpsee::MethodResponse;
use jsonrpsee::ResponsePayload;
use jsonrpsee::core::middleware::ResponseFuture;
use jsonrpsee::types::ErrorObjectOwned;
use jsonrpsee::types::Id;
use revm::context::DBErrorMarker;
use stratus_macros::ErrorCode;

use crate::alias::JsonValue;
use crate::eth::executor::ExecutorError;
use crate::eth::follower::ConsensusError;
use crate::eth::follower::ImporterError;
use crate::eth::rpc::MulticallError;
use crate::eth::rpc::RpcError;
use crate::eth::storage::StorageError;
use crate::ext::to_json_value;

pub trait ErrorCode {
    fn error_code(&self) -> i32;
    fn str_repr_from_err_code(code: i32) -> Option<&'static str>;
}

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr, ErrorCode)]
#[major_error_code = 6000]
pub enum UnexpectedError {
    #[error("unexpected channel {channel} closed.")]
    #[error_code = 1]
    ChannelClosed { channel: &'static str },

    #[error("unexpected error: {0:?}.")]
    #[error_code = 2]
    Unexpected(anyhow::Error),
}

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr, ErrorCode)]
#[major_error_code = 7000]
pub enum StateError {
    #[error("stratus is not ready to start servicing requests.")]
    #[error_code = 1]
    StratusNotReady,

    #[error("stratus is shutting down.")]
    #[error_code = 2]
    StratusShutdown,

    #[error("stratus node is not a follower.")]
    #[error_code = 3]
    StratusNotFollower,

    #[error("incorrect password, cancelling operation.")]
    #[error_code = 4]
    InvalidPassword,

    #[error("stratus node is already in the process of changing mode.")]
    #[error_code = 5]
    ModeChangeInProgress,

    #[error("transaction processing is temporarily disabled.")]
    #[error_code = 6]
    TransactionsDisabled,

    #[error("can't change miner mode while transactions are enabled.")]
    #[error_code = 7]
    TransactionsEnabled,

    #[error("operation cannot be performed with current configuration.")]
    #[error_code = 8]
    Misconfigured { details: &'static str },
}

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr)]
pub enum StratusError {
    #[error(transparent)]
    RPC(#[from] RpcError),

    #[error(transparent)]
    Executor(#[from] ExecutorError),

    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error(transparent)]
    Importer(#[from] ImporterError),

    #[error(transparent)]
    Consensus(#[from] ConsensusError),

    #[error(transparent)]
    Unexpected(#[from] UnexpectedError),

    #[error(transparent)]
    State(#[from] StateError),

    #[error(transparent)]
    Multicall(#[from] MulticallError),
}

impl ErrorCode for StratusError {
    fn error_code(&self) -> i32 {
        match self {
            Self::RPC(err) => err.error_code(),
            Self::Executor(err) => err.error_code(),
            Self::Storage(err) => err.error_code(),
            Self::Importer(err) => err.error_code(),
            Self::Consensus(err) => err.error_code(),
            Self::Unexpected(err) => err.error_code(),
            Self::State(err) => err.error_code(),
            Self::Multicall(err) => err.error_code(),
        }
    }

    fn str_repr_from_err_code(code: i32) -> Option<&'static str> {
        let major = code / 1000;
        match major {
            1 => RpcError::str_repr_from_err_code(code),
            2 => ExecutorError::str_repr_from_err_code(code),
            3 => StorageError::str_repr_from_err_code(code),
            4 => ImporterError::str_repr_from_err_code(code),
            5 => ConsensusError::str_repr_from_err_code(code),
            6 => UnexpectedError::str_repr_from_err_code(code),
            7 => StateError::str_repr_from_err_code(code),
            8 => MulticallError::str_repr_from_err_code(code),
            _ => None,
        }
    }
}

impl DBErrorMarker for StratusError {}

impl StratusError {
    /// Error message to be used in JSON-RPC response.
    pub fn rpc_message(&self) -> String {
        self.to_string()
    }

    /// Error additional data to be used in JSON-RPC response.
    pub fn rpc_data(&self) -> JsonValue {
        match self {
            // RPC
            Self::RPC(RpcError::BlockFilterInvalid { filter }) => to_json_value(filter),
            Self::RPC(RpcError::ParameterDecodeError { decode_error, .. }) => to_json_value(decode_error),
            Self::RPC(RpcError::ClientBlocked { client }) => to_json_value(client),

            // Transaction
            Self::RPC(RpcError::TransactionInvalid { decode_error }) => to_json_value(decode_error),
            Self::Executor(ExecutorError::EvmFailed(e)) => JsonValue::String(e.to_string()),
            Self::Executor(ExecutorError::RevertedCall { output }) => to_json_value(output),
            Self::Executor(ExecutorError::RevertedCallWithReason { reason }) => to_json_value(reason),

            // Unexpected
            Self::Unexpected(UnexpectedError::Unexpected(e)) => JsonValue::String(e.to_string()),

            // Multicall
            Self::Multicall(e) => JsonValue::String(e.to_string()),

            _ => JsonValue::Null,
        }
    }

    pub fn to_response_future<'a>(self, id: Id<'_>) -> ResponseFuture<BoxFuture<'a, MethodResponse>, MethodResponse> {
        let error: ErrorObjectOwned = self.into();
        let response = ResponsePayload::<()>::error(error);
        let method_response = MethodResponse::response(id, response, u32::MAX as usize);
        ResponseFuture::ready(method_response)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl<T> From<crossbeam_channel::SendError<T>> for StratusError {
    fn from(_: crossbeam_channel::SendError<T>) -> Self {
        Self::Unexpected(UnexpectedError::ChannelClosed { channel: "unkown" })
    }
}

impl From<anyhow::Error> for StratusError {
    fn from(value: anyhow::Error) -> Self {
        Self::Unexpected(UnexpectedError::Unexpected(value))
    }
}

impl From<serde_json::Error> for StratusError {
    fn from(value: serde_json::Error) -> Self {
        Self::Unexpected(UnexpectedError::Unexpected(anyhow::anyhow!(value)))
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<StratusError> for ErrorObjectOwned {
    fn from(value: StratusError) -> Self {
        // return response from leader
        if let StratusError::Executor(ExecutorError::LeaderFailed(response)) = value {
            return response;
        }
        // generate response
        let data = match value.rpc_data() {
            serde_json::Value::String(data_str) => {
                let data_str = data_str.trim_start_matches('\"').trim_end_matches('\"').replace("\\\"", "\"");
                JsonValue::String(data_str)
            }
            data => data,
        };

        Self::owned(value.error_code(), value.rpc_message(), Some(data))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeInputError {
    #[error("Input too short: {message}")]
    InputTooShort { message: String },

    #[error("Function unknown: {message}")]
    FunctionUnknown { message: String },

    #[error("Invalid input: {message}")]
    InvalidAbi { message: String },

    #[error("Invalid ABI: {source}")]
    InvalidInput {
        #[from]
        source: alloy_dyn_abi::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_str_repr_from_err_code() {
        // RPC error
        assert_eq!(StratusError::str_repr_from_err_code(1001), Some("BlockFilterInvalid"));

        // Transaction error
        assert_eq!(StratusError::str_repr_from_err_code(2001), Some("AccountNotContract"));

        // Storage error
        assert_eq!(StratusError::str_repr_from_err_code(3001), Some("BlockConflict"));

        // Importer error
        assert_eq!(StratusError::str_repr_from_err_code(4001), Some("AlreadyRunning"));

        // Consensus error
        assert_eq!(StratusError::str_repr_from_err_code(5001), Some("Unavailable"));

        // Unexpected error
        assert_eq!(StratusError::str_repr_from_err_code(6001), Some("ChannelClosed"));

        // State error
        assert_eq!(StratusError::str_repr_from_err_code(7003), Some("StratusNotFollower"));

        // Invalid error code
        assert_eq!(StratusError::str_repr_from_err_code(9999), None);
    }
}
