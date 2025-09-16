use futures::future::BoxFuture;
use jsonrpsee::MethodResponse;
use jsonrpsee::ResponsePayload;
use jsonrpsee::core::middleware::ResponseFuture;
use jsonrpsee::types::ErrorObjectOwned;
use jsonrpsee::types::Id;
use revm::context::DBErrorMarker;
use revm::context::result::EVMError;
use stratus_macros::ErrorCode;

use super::execution_result::RevertReason;
use crate::alias::JsonValue;
use crate::eth::executor::EvmInput;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ExecutionConflicts;
use crate::eth::primitives::Nonce;
use crate::ext::to_json_value;

pub trait ErrorCode {
    fn error_code(&self) -> i32;
    fn str_repr_from_err_code(code: i32) -> Option<&'static str>;
}

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr, ErrorCode)]
#[major_error_code = 1000]
pub enum RpcError {
    #[error("Block filter does not point to a valid block.")]
    #[error_code = 1]
    BlockFilterInvalid { filter: BlockFilter },

    #[error("Denied because will fetch data from {actual} blocks, but the max allowed is {max}.")]
    #[error_code = 2]
    BlockRangeInvalid { actual: u64, max: u64 },

    #[error("Denied because client did not identify itself.")]
    #[error_code = 3]
    ClientMissing,

    #[error("Failed to decode {rust_type} parameter.")]
    #[error_code = 4]
    ParameterInvalid { rust_type: &'static str, decode_error: String },

    #[error("Expected {rust_type} parameter, but received nothing.")]
    #[error_code = 5]
    ParameterMissing { rust_type: &'static str },

    #[error("Invalid subscription event: {event}")]
    #[error_code = 6]
    SubscriptionInvalid { event: String },

    #[error("Denied because reached maximum subscription limit of {max}.")]
    #[error_code = 7]
    SubscriptionLimit { max: u32 },

    #[error("Failed to decode transaction RLP data.")]
    #[error_code = 8]
    TransactionInvalid { decode_error: String },

    #[error("Miner mode param is invalid.")]
    #[error_code = 9]
    MinerModeParamInvalid,
}

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr, ErrorCode)]
#[major_error_code = 2000]
pub enum TransactionError {
    #[error("Account at {address} is not a contract.")]
    #[error_code = 1]
    AccountNotContract { address: Address },

    #[error("Transaction nonce {transaction} does not match account nonce {account}.")]
    #[error_code = 2]
    Nonce { transaction: Nonce, account: Nonce },

    #[error("Failed to execute transaction in EVM: {0:?}.")]
    #[error_code = 3]
    EvmFailed(String), // TODO: split this in multiple errors

    #[error("Failed to execute transaction in leader: {0:?}.")]
    #[error_code = 4]
    LeaderFailed(ErrorObjectOwned),

    #[error("Failed to forward transaction to leader node.")]
    #[error_code = 5]
    ForwardToLeaderFailed,

    #[error("Transaction reverted during execution.")]
    #[error_code = 6]
    RevertedCall { output: Bytes },

    #[error("Transaction from zero address is not allowed.")]
    #[error_code = 7]
    FromZeroAddress,

    #[error("Transaction reverted during execution.")]
    #[error_code = 8]
    RevertedCallWithReason { reason: RevertReason },
}

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr, ErrorCode)]
#[major_error_code = 3000]
pub enum StorageError {
    #[error("Block conflict: {number} already exists in the permanent storage.")]
    #[error_code = 1]
    BlockConflict { number: BlockNumber },

    #[error("Mined number conflict between new block number ({new}) and mined block number ({mined}).")]
    #[error_code = 2]
    MinedNumberConflict { new: BlockNumber, mined: BlockNumber },

    #[error("Transaction execution conflicts: {0:?}.")]
    #[error_code = 3]
    TransactionConflict(Box<ExecutionConflicts>),

    #[error("Transaction input does not match block header")]
    #[error_code = 4]
    EvmInputMismatch { expected: Box<EvmInput>, actual: Box<EvmInput> },

    #[error("Pending number conflict between new block number ({new}) and pending block number ({pending}).")]
    #[error_code = 5]
    PendingNumberConflict { new: BlockNumber, pending: BlockNumber },

    #[error("There are ({pending_txs}) pending transactions.")]
    #[error_code = 6]
    PendingTransactionsExist { pending_txs: usize },

    #[error("Rocksdb returned an error: {err}")]
    #[error_code = 7]
    RocksError { err: anyhow::Error },

    #[error("Block not found using filter: {filter}")]
    #[error_code = 8]
    BlockNotFound { filter: BlockFilter },

    #[error("Unexpected storage error: {msg}")]
    #[error_code = 9]
    Unexpected { msg: String },
}

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr, ErrorCode)]
#[major_error_code = 4000]
pub enum ImporterError {
    #[error("Importer is already running.")]
    #[error_code = 1]
    AlreadyRunning,

    #[error("Importer is already shutdown.")]
    #[error_code = 2]
    AlreadyShutdown,

    #[error("Failed to parse importer configuration.")]
    #[error_code = 3]
    ConfigParseError,

    #[error("Failed to initialize importer.")]
    #[error_code = 4]
    InitError,
}

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr, ErrorCode)]
#[major_error_code = 5000]
pub enum ConsensusError {
    #[error("Consensus is temporarily unavailable for follower node.")]
    #[error_code = 1]
    Unavailable,

    #[error("Consensus is set.")]
    #[error_code = 2]
    Set,

    #[error("Failed to update consensus: Consensus is not set.")]
    #[error_code = 3]
    NotSet,
}

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr, ErrorCode)]
#[major_error_code = 6000]
pub enum UnexpectedError {
    #[error("Unexpected channel {channel} closed.")]
    #[error_code = 1]
    ChannelClosed { channel: &'static str },

    #[error("Unexpected error: {0:?}.")]
    #[error_code = 2]
    Unexpected(anyhow::Error),
}

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr, ErrorCode)]
#[major_error_code = 7000]
pub enum StateError {
    #[error("Stratus is not ready to start servicing requests.")]
    #[error_code = 1]
    StratusNotReady,

    #[error("Stratus is shutting down.")]
    #[error_code = 2]
    StratusShutdown,

    #[error("Stratus node is not a follower.")]
    #[error_code = 3]
    StratusNotFollower,

    #[error("Incorrect password, cancelling operation.")]
    #[error_code = 4]
    InvalidPassword,

    #[error("Stratus node is already in the process of changing mode.")]
    #[error_code = 5]
    ModeChangeInProgress,

    #[error("Transaction processing is temporarily disabled.")]
    #[error_code = 6]
    TransactionsDisabled,

    #[error("Can't change miner mode while transactions are enabled.")]
    #[error_code = 7]
    TransactionsEnabled,
}

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr)]
pub enum StratusError {
    #[error(transparent)]
    RPC(#[from] RpcError),

    #[error(transparent)]
    Transaction(#[from] TransactionError),

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
}

impl ErrorCode for StratusError {
    fn error_code(&self) -> i32 {
        match self {
            Self::RPC(err) => err.error_code(),
            Self::Transaction(err) => err.error_code(),
            Self::Storage(err) => err.error_code(),
            Self::Importer(err) => err.error_code(),
            Self::Consensus(err) => err.error_code(),
            Self::Unexpected(err) => err.error_code(),
            Self::State(err) => err.error_code(),
        }
    }

    fn str_repr_from_err_code(code: i32) -> Option<&'static str> {
        let major = code / 1000;
        match major {
            1 => RpcError::str_repr_from_err_code(code),
            2 => TransactionError::str_repr_from_err_code(code),
            3 => StorageError::str_repr_from_err_code(code),
            4 => ImporterError::str_repr_from_err_code(code),
            5 => ConsensusError::str_repr_from_err_code(code),
            6 => UnexpectedError::str_repr_from_err_code(code),
            7 => StateError::str_repr_from_err_code(code),
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
            Self::RPC(RpcError::ParameterInvalid { decode_error, .. }) => to_json_value(decode_error),

            // Transaction
            Self::RPC(RpcError::TransactionInvalid { decode_error }) => to_json_value(decode_error),
            Self::Transaction(TransactionError::EvmFailed(e)) => JsonValue::String(e.to_string()),
            Self::Transaction(TransactionError::RevertedCall { output }) => to_json_value(output),
            Self::Transaction(TransactionError::RevertedCallWithReason { reason }) => to_json_value(reason),

            // Unexpected
            Self::Unexpected(UnexpectedError::Unexpected(e)) => JsonValue::String(e.to_string()),

            _ => JsonValue::Null,
        }
    }

    pub fn to_response_future<'a>(self, id: Id<'_>) -> ResponseFuture<BoxFuture<'a, MethodResponse>, MethodResponse> {
        let response = ResponsePayload::<()>::error(StratusError::RPC(RpcError::ClientMissing));
        let method_response = MethodResponse::response(id, response, u32::MAX as usize);
        ResponseFuture::ready(method_response)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

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

impl From<EVMError<StratusError>> for StratusError {
    fn from(value: EVMError<StratusError>) -> Self {
        match value {
            EVMError::Database(err) => err,
            EVMError::Custom(err) => Self::Transaction(TransactionError::EvmFailed(err)),
            EVMError::Header(err) => Self::Transaction(TransactionError::EvmFailed(err.to_string())),
            EVMError::Transaction(err) => Self::Transaction(TransactionError::EvmFailed(err.to_string())),
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<StratusError> for ErrorObjectOwned {
    fn from(value: StratusError) -> Self {
        // return response from leader
        if let StratusError::Transaction(TransactionError::LeaderFailed(response)) = value {
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
