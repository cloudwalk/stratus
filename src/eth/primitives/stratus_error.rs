use jsonrpsee::types::error::CALL_EXECUTION_FAILED_CODE;
use jsonrpsee::types::error::INTERNAL_ERROR_CODE;
use jsonrpsee::types::error::INVALID_PARAMS_CODE;
use jsonrpsee::types::error::INVALID_REQUEST_CODE;
use jsonrpsee::types::error::SERVER_IS_BUSY_CODE;
use jsonrpsee::types::ErrorObjectOwned;
use strum::EnumProperty;

use crate::alias::JsonValue;
use crate::eth::executor::EvmInput;
use crate::eth::primitives::Address;
use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ExecutionConflicts;
use crate::eth::primitives::Nonce;
use crate::ext::to_json_value;

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr)]
pub enum RpcError {
    #[error("Block filter does not point to a valid block.")]
    #[strum(props(kind = "client_request"))]
    BlockFilterInvalid { filter: BlockFilter },

    #[error("Denied because will fetch data from {actual} blocks, but the max allowed is {max}.")]
    #[strum(props(kind = "client_request"))]
    BlockRangeInvalid { actual: u64, max: u64 },

    #[error("Denied because client did not identify itself.")]
    #[strum(props(kind = "client_request"))]
    ClientMissing,

    #[error("Failed to decode {rust_type} parameter.")]
    #[strum(props(kind = "client_request"))]
    ParameterInvalid { rust_type: &'static str, decode_error: String },

    #[error("Expected {rust_type} parameter, but received nothing.")]
    #[strum(props(kind = "client_request"))]
    ParameterMissing { rust_type: &'static str },

    #[error("Invalid subscription event: {event}")]
    #[strum(props(kind = "client_request"))]
    SubscriptionInvalid { event: String },

    #[error("Denied because reached maximum subscription limit of {max}.")]
    #[strum(props(kind = "client_state"))]
    SubscriptionLimit { max: u32 },

    #[error("Failed to decode transaction RLP data.")]
    #[strum(props(kind = "client_request"))]
    TransactionInvalid { decode_error: String },

    #[error("Miner mode param is invalid.")]
    #[strum(props(kind = "internal"))]
    MinerModeParamInvalid,
}

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr)]
pub enum TransactionError {
    #[error("Account at {address} is not a contract.")]
    #[strum(props(kind = "execution"))]
    AccountNotContract { address: Address },

    #[error("Transaction nonce {transaction} does not match account nonce {account}.")]
    #[strum(props(kind = "execution"))]
    Nonce { transaction: Nonce, account: Nonce },

    #[error("Failed to executed transaction in EVM: {0:?}.")]
    #[strum(props(kind = "execution"))]
    EvmFailed(String), // TODO: split this in multiple errors

    #[error("Failed to execute transaction in leader: {0:?}.")]
    #[strum(props(kind = "execution"))]
    LeaderFailed(ErrorObjectOwned),

    #[error("Failed to forward transaction to leader node.")]
    #[strum(props(kind = "execution"))]
    ForwardToLeaderFailed,

    #[error("Transaction reverted during execution.")]
    #[strum(props(kind = "execution"))]
    Reverted { output: Bytes },

    #[error("Transaction from zero address is not allowed.")]
    #[strum(props(kind = "execution"))]
    FromZeroAddress,
}

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr)]
pub enum StorageError {
    #[error("Block conflict: {number} already exists in the permanent storage.")]
    #[strum(props(kind = "internal"))]
    BlockConflict { number: BlockNumber },

    #[error("Mined number conflict between new block number ({new}) and mined block number ({mined}).")]
    #[strum(props(kind = "internal"))]
    MinedNumberConflict { new: BlockNumber, mined: BlockNumber },

    #[error("Transaction execution conflicts: {0:?}.")]
    #[strum(props(kind = "execution"))]
    TransactionConflict(Box<ExecutionConflicts>),

    #[error("Transaction input does not match block header")]
    #[strum(props(kind = "execution"))]
    EvmInputMismatch { expected: Box<EvmInput>, actual: Box<EvmInput> },

    #[error("Pending number conflict between new block number ({new}) and pending block number ({pending}).")]
    #[strum(props(kind = "internal"))]
    PendingNumberConflict { new: BlockNumber, pending: BlockNumber },

    #[error("There are ({pending_txs}) pending transactions.")]
    #[strum(props(kind = "internal"))]
    PendingTransactionsExist { pending_txs: usize },

    #[error("Rocksdb returned an error: {err}")]
    #[strum(props(kind = "internal"))]
    RocksError { err: anyhow::Error },

    #[error("Block not found using filter: {filter}")]
    #[strum(props(kind = "internal"))]
    BlockNotFound { filter: BlockFilter },

    #[error("Unexpected storage error: {msg}")]
    #[strum(props(kind = "internal"))]
    Unexpected { msg: String },
}

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr)]
pub enum ImporterError {
    #[error("Importer is already running.")]
    #[strum(props(kind = "internal"))]
    AlreadyRunning,

    #[error("Importer is already shutdown.")]
    #[strum(props(kind = "internal"))]
    AlreadyShutdown,

    #[error("Failed to parse importer configuration.")]
    #[strum(props(kind = "client_request"))]
    ConfigParseError,

    #[error("Failed to initialize importer.")]
    #[strum(props(kind = "internal"))]
    InitError,
}

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr)]
pub enum ConsensusError {
    #[error("Consensus is temporarily unavailable for follower node.")]
    #[strum(props(kind = "internal"))]
    Unavailable,

    #[error("Consensus is set.")]
    #[strum(props(kind = "internal"))]
    Set,

    #[error("Failed to update consensus: Consensus is not set.")]
    #[strum(props(kind = "internal"))]
    NotSet,
}

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr)]
pub enum UnexpectedError {
    #[error("Unexpected channel {channel} closed.")]
    #[strum(props(kind = "internal"))]
    ChannelClosed { channel: &'static str },

    #[error("Unexpected error: {0:?}.")]
    #[strum(props(kind = "internal"))]
    Unexpected(anyhow::Error),
}

#[derive(Debug, thiserror::Error, strum::EnumProperty, strum::IntoStaticStr)]
pub enum StateError {
    #[error("Stratus is not ready to start servicing requests.")]
    #[strum(props(kind = "server_state"))]
    StratusNotReady,

    #[error("Stratus is shutting down.")]
    #[strum(props(kind = "server_state"))]
    StratusShutdown,

    #[error("Stratus node is not a follower.")]
    #[strum(props(kind = "server_state"))]
    StratusNotFollower,

    #[error("Incorrect password, cancelling operation.")]
    #[strum(props(kind = "server_state"))]
    InvalidPassword,

    #[error("Stratus node is already in the process of changing mode.")]
    #[strum(props(kind = "server_state"))]
    ModeChangeInProgress,

    #[error("Transaction processing is temporarily disabled.")]
    #[strum(props(kind = "server_state"))]
    TransactionsDisabled,

    #[error("Can't change miner mode while transactions are enabled.")]
    #[strum(props(kind = "server_state"))]
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

impl StratusError {
    /// Checks if the error is an unexpected/internal error.
    pub fn is_internal(&self) -> bool {
        self.rpc_code() == INTERNAL_ERROR_CODE
    }

    /// Error code to be used in JSON-RPC response.
    pub fn rpc_code(&self) -> i32 {
        match self.get_str("kind") {
            Some("client_request") => INVALID_PARAMS_CODE,
            Some("client_state") => INVALID_REQUEST_CODE,
            Some("server_state") => SERVER_IS_BUSY_CODE,
            Some("execution") => CALL_EXECUTION_FAILED_CODE,
            Some("internal") => INTERNAL_ERROR_CODE,
            Some(kind) => {
                tracing::warn!(kind, "stratus error with unhandled kind");
                INTERNAL_ERROR_CODE
            }
            None => {
                tracing::warn!("stratus error without kind");
                INTERNAL_ERROR_CODE
            }
        }
    }

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
            Self::Transaction(TransactionError::Reverted { output }) => to_json_value(output),

            // Unexpected
            Self::Unexpected(UnexpectedError::Unexpected(e)) => JsonValue::String(e.to_string()),

            _ => JsonValue::Null,
        }
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

        Self::owned(value.rpc_code(), value.rpc_message(), Some(data))
    }
}
