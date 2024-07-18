use crate::eth::primitives::BlockFilter;
use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExecutionConflicts;

#[derive(Debug, thiserror::Error, strum::EnumIs)]
pub enum StorageError {
    /// Failure to convert a block filter to a point in time.
    #[error("Block filter {filter} points to a invalid block.")]
    InvalidPointInTime { filter: BlockFilter },

    /// Conflict between transaction execution being saved and current storage state.
    #[error("Execution conflict: {0:?}")]
    ExecutionConflict(Box<ExecutionConflicts>),

    /// State conflict between block being saved and block in the permanent storage.
    #[error("Block conflict: {number} already exists in the permanent storage.")]
    BlockConflict { number: BlockNumber },

    /// State conflict between block being saved and current mined block number.
    #[error("Mined number conflict between new block number ({new}) and mined block number ({mined}).")]
    MinedNumberConflict { new: BlockNumber, mined: BlockNumber },

    /// State conflict between block being saved and current pending block number.
    #[error("Pending number conflict between new block number ({new}) and pending block number ({pending}).")]
    PendingNumberConflict { new: BlockNumber, pending: BlockNumber },

    /// Generic error interacting with the storage.
    #[error("Unexpected storage error: {0}")]
    Unexpected(#[from] anyhow::Error),
}
