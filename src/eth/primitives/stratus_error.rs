use jsonrpsee::types::error::CALL_EXECUTION_FAILED_CODE;
use jsonrpsee::types::error::INTERNAL_ERROR_CODE;
use jsonrpsee::types::error::INVALID_PARAMS_CODE;
use jsonrpsee::types::error::INVALID_REQUEST_CODE;
use jsonrpsee::types::error::SERVER_IS_BUSY_CODE;
use jsonrpsee::types::error::TOO_MANY_SUBSCRIPTIONS_CODE;
use jsonrpsee::types::ErrorObjectOwned;
use revm::primitives::EVMError;

use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::Bytes;
use crate::eth::primitives::ExecutionConflicts;
use crate::ext::to_json_string;

#[derive(Debug, thiserror::Error)]
pub enum StratusError {
    // RPC
    #[error("Block filter does not point to a valid block.")]
    RpcBlockFilterInvalid { filter: BlockFilter },

    #[error("Denied because will fetch data from {actual} blocks, but the max allowed is {max}.")]
    RpcBlockRangeInvalid { actual: u64, max: u64 },

    #[error("Denied because client did not identify itself.")]
    RpcClientMissing,

    #[error("Failed to decode {rust_type} parameter.")]
    RpcParameterInvalid { rust_type: &'static str, decode_error: String },

    #[error("Expected {rust_type} parameter, but received nothing.")]
    RpcParameterMissing { rust_type: &'static str },

    #[error("Invalid subscription event: {event}")]
    RpcSubscriptionInvalid { event: String },

    #[error("Denied because reached maximum subscription limit of {max}.")]
    RpcSubscriptionLimit { max: u32 },

    #[error("Transaction processing is temporarily disabled.")]
    RpcTransactionDisabled,

    #[error("Failed to decode transaction RLP data.")]
    RpcTransactionInvalid { decode_error: String },

    // Transaction
    #[error("Transaction execution conflicts: {0:?}.")]
    TransactionConflict(Box<ExecutionConflicts>),

    #[error("Failed to executed transaction in EVM: {0:?}.")]
    TransactionFailed(EVMError<anyhow::Error>), // split this in multiple errors

    #[error("Failed to forward transaction to leader node.")]
    TransactionForwardToLeaderFailed,

    #[error("Transaction reverted during execution.")]
    TransactionReverted { output: Bytes },

    // Storage
    #[error("Block conflict: {number} already exists in the permanent storage.")]
    StorageBlockConflict { number: BlockNumber },

    #[error("Mined number conflict between new block number ({new}) and mined block number ({mined}).")]
    StorageMinedNumberConflict { new: BlockNumber, mined: BlockNumber },

    #[error("Pending number conflict between new block number ({new}) and pending block number ({pending}).")]
    StoragePendingNumberConflict { new: BlockNumber, pending: BlockNumber },

    // Unexpected
    #[error("Unexpected channel {channel} closed.")]
    UnexpectedChannelClosed { channel: &'static str },

    #[error("Unexpected error: {0:?}.")]
    Unexpected(anyhow::Error),

    // Stratus state
    #[error("Stratus is not ready to start servicing requests.")]
    StratusNotReady,

    #[error("Stratus is shutting down.")]
    StratusShutdown,
}

impl StratusError {
    /// Error code to be used in JSON-RPC response.
    pub fn rpc_code(&self) -> i32 {
        match self {
            // RPC Request
            Self::RpcClientMissing => INVALID_REQUEST_CODE,

            // RPC Params
            Self::RpcBlockFilterInvalid { .. } => INVALID_PARAMS_CODE,
            Self::RpcBlockRangeInvalid { .. } => INVALID_PARAMS_CODE,
            Self::RpcParameterInvalid { .. } => INVALID_PARAMS_CODE,
            Self::RpcParameterMissing { .. } => INVALID_PARAMS_CODE,
            Self::RpcSubscriptionInvalid { .. } => INVALID_PARAMS_CODE,
            Self::RpcSubscriptionLimit { .. } => TOO_MANY_SUBSCRIPTIONS_CODE,
            Self::RpcTransactionInvalid { .. } => INVALID_PARAMS_CODE,

            // Execution
            Self::RpcTransactionDisabled => INTERNAL_ERROR_CODE,
            Self::TransactionForwardToLeaderFailed => INTERNAL_ERROR_CODE,
            Self::TransactionReverted { .. } => CALL_EXECUTION_FAILED_CODE,
            Self::TransactionConflict(_) => INTERNAL_ERROR_CODE,

            // EVM
            Self::TransactionFailed(_) => INTERNAL_ERROR_CODE,

            // Storage
            Self::StorageBlockConflict { .. } => INTERNAL_ERROR_CODE,
            Self::StorageMinedNumberConflict { .. } => INTERNAL_ERROR_CODE,
            Self::StoragePendingNumberConflict { .. } => INTERNAL_ERROR_CODE,

            // Unexpected
            Self::Unexpected(_) => INTERNAL_ERROR_CODE,
            Self::UnexpectedChannelClosed { .. } => INTERNAL_ERROR_CODE,

            // Stratus state
            Self::StratusNotReady => SERVER_IS_BUSY_CODE,
            Self::StratusShutdown => SERVER_IS_BUSY_CODE,
        }
    }

    /// Error message to be used in JSON-RPC response.
    pub fn rpc_message(&self) -> String {
        self.to_string()
    }

    /// Error additional data to be used in JSON-RPC response.
    pub fn rpc_data(&self) -> Option<String> {
        match self {
            // RPC
            Self::RpcBlockFilterInvalid { filter } => Some(to_json_string(filter)),
            Self::RpcParameterInvalid { decode_error, .. } => Some(decode_error.to_string()),

            // Transaction
            Self::RpcTransactionInvalid { decode_error } => Some(decode_error.to_string()),
            Self::TransactionFailed(e) => Some(e.to_string()),
            Self::TransactionReverted { output } => Some(const_hex::encode_prefixed(output)),

            // Unexpected
            Self::Unexpected(error) => Some(error.to_string()),
            _ => None,
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
        Self::owned(value.rpc_code(), value.rpc_message(), value.rpc_data())
    }
}
