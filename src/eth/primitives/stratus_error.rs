use jsonrpsee::types::error::CALL_EXECUTION_FAILED_CODE;
use jsonrpsee::types::error::INTERNAL_ERROR_CODE;
use jsonrpsee::types::error::INVALID_PARAMS_CODE;
use jsonrpsee::types::error::INVALID_REQUEST_CODE;
use jsonrpsee::types::error::SERVER_IS_BUSY_CODE;
use jsonrpsee::types::ErrorObjectOwned;
use revm::primitives::EVMError;
use strum::EnumProperty;

use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ExecutionConflicts;
use crate::eth::primitives::Nonce;
use crate::ext::to_json_value;
use crate::alias::JsonValue;

/// Valid  error catogories are:
/// * client_request: request is invalid.
/// * client_state:   request is valid, specific client rules rejects it.
/// * server_state:   request is valid, global server rules rejects it.
/// * execution:      request is valid, but failed in executor/evm.
/// * internal:       request is valid, but a an internal component failed.
#[derive(Debug, thiserror::Error, strum::EnumProperty)]
pub enum StratusError {
    // -------------------------------------------------------------------------
    // RPC
    // -------------------------------------------------------------------------
    #[error("Block filter does not point to a valid block.")]
    #[strum(props(kind = "client_request"))]
    RpcBlockFilterInvalid { filter: BlockFilter },

    #[error("Denied because will fetch data from {actual} blocks, but the max allowed is {max}.")]
    #[strum(props(kind = "client_request"))]
    RpcBlockRangeInvalid { actual: u64, max: u64 },

    #[error("Denied because client did not identify itself.")]
    #[strum(props(kind = "client_request"))]
    RpcClientMissing,

    #[error("Failed to decode {rust_type} parameter.")]
    #[strum(props(kind = "client_request"))]
    RpcParameterInvalid { rust_type: &'static str, decode_error: String },

    #[error("Expected {rust_type} parameter, but received nothing.")]
    #[strum(props(kind = "client_request"))]
    RpcParameterMissing { rust_type: &'static str },

    #[error("Invalid subscription event: {event}")]
    #[strum(props(kind = "client_request"))]
    RpcSubscriptionInvalid { event: String },

    #[error("Denied because reached maximum subscription limit of {max}.")]
    #[strum(props(kind = "client_state"))]
    RpcSubscriptionLimit { max: u32 },

    #[error("Transaction processing is temporarily disabled.")]
    #[strum(props(kind = "server_state"))]
    RpcTransactionDisabled,

    #[error("Failed to decode transaction RLP data.")]
    #[strum(props(kind = "client_request"))]
    RpcTransactionInvalid { decode_error: String },

    // -------------------------------------------------------------------------
    // Transaction
    // -------------------------------------------------------------------------
    #[error("Transaction execution conflicts: {0:?}.")]
    #[strum(props(kind = "execution"))]
    TransactionConflict(Box<ExecutionConflicts>),

    #[error("Transaction nonce {transaction} does not match account nonce {account}.")]
    #[strum(props(kind = "execution"))]
    TransactionNonce { transaction: Nonce, account: Nonce },

    #[error("Failed to executed transaction in EVM: {0:?}.")]
    #[strum(props(kind = "execution"))]
    TransactionFailed(EVMError<anyhow::Error>), // split this in multiple errors

    #[error("Failed to forward transaction to leader node.")]
    #[strum(props(kind = "execution"))]
    TransactionForwardToLeaderFailed,

    #[error("Transaction reverted during execution.")]
    #[strum(props(kind = "execution"))]
    TransactionReverted { output: Bytes },

    #[error("Transaction from zero address is not allowed.")]
    #[strum(props(kind = "execution"))]
    TransactionFromZeroAddress,

    // -------------------------------------------------------------------------
    // Storage
    // -------------------------------------------------------------------------
    #[error("Block conflict: {number} already exists in the permanent storage.")]
    #[strum(props(kind = "internal"))]
    StorageBlockConflict { number: BlockNumber },

    #[error("Mined number conflict between new block number ({new}) and mined block number ({mined}).")]
    #[strum(props(kind = "internal"))]
    StorageMinedNumberConflict { new: BlockNumber, mined: BlockNumber },

    #[error("Pending number conflict between new block number ({new}) and pending block number ({pending}).")]
    #[strum(props(kind = "internal"))]
    StoragePendingNumberConflict { new: BlockNumber, pending: BlockNumber },

    // -------------------------------------------------------------------------
    // Unexpected
    // -------------------------------------------------------------------------
    #[error("Unexpected channel {channel} closed.")]
    #[strum(props(kind = "internal"))]
    UnexpectedChannelClosed { channel: &'static str },

    #[error("Unexpected error: {0:?}.")]
    #[strum(props(kind = "internal"))]
    Unexpected(anyhow::Error),

    // -------------------------------------------------------------------------
    // Stratus state
    // -------------------------------------------------------------------------
    #[error("Stratus is not ready to start servicing requests.")]
    #[strum(props(kind = "server_state"))]
    StratusNotReady,

    #[error("Stratus is shutting down.")]
    #[strum(props(kind = "server_state"))]
    StratusShutdown,
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
                tracing::warn!("stratus error with unhandled kind: {kind}");
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
            Self::RpcBlockFilterInvalid { filter } => to_json_value(filter),
            Self::RpcParameterInvalid { decode_error, .. } => to_json_value(decode_error),

            // Transaction
            Self::RpcTransactionInvalid { decode_error } => to_json_value(decode_error),
            Self::TransactionFailed(e) => JsonValue::String(e.to_string()),
            Self::TransactionReverted { output } => to_json_value(output),

            // Unexpected
            Self::Unexpected(e) => JsonValue::String(e.to_string()),

            _ => JsonValue::Null,
        }
    }
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------

impl From<anyhow::Error> for StratusError {
    fn from(value: anyhow::Error) -> Self {
        Self::Unexpected(value)
    }
}

// -----------------------------------------------------------------------------
// Conversions: Self -> Other
// -----------------------------------------------------------------------------
impl From<StratusError> for ErrorObjectOwned {
    fn from(value: StratusError) -> Self {
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
