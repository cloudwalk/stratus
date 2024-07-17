use crate::eth::primitives::BlockNumber;
use crate::eth::primitives::ExecutionConflicts;

#[derive(Debug, thiserror::Error, strum::EnumIs, derive_new::new)]
pub enum StorageError {
    /// Generic error interacting with the storage.
    #[error("Storage error: {0}")]
    Generic(#[from] anyhow::Error),

    /// State conflict between transaction execution and current storage state.
    #[error("Storage conflict: {0:?}")]
    Conflict(ExecutionConflicts),

    /// State conflict between block being saved and block in the permanent storage.
    #[error("Block with number {number} already exists.")]
    MinedBlockExists { number: BlockNumber },

    /// State conflict between block being saved and current mined block number.
    #[error("Mismatch between new block number ({new}) and mined block number ({mined}).")]
    MinedNumberMismatch { new: BlockNumber, mined: BlockNumber },

    /// State conflict between block being saved and current pending block number.
    #[error("Mismatch between new block number ({new}) and pending block number ({pending}).")]
    PendingNumberMismatch { new: BlockNumber, pending: BlockNumber },
}
