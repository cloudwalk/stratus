use crate::eth::primitives::ExecutionConflicts;

#[derive(Debug, thiserror::Error, strum::EnumIs, derive_new::new)]
pub enum StorageError {
    /// Generic error interacting with the storage.
    #[error("Storage error: {0}")]
    Generic(#[from] anyhow::Error),

    /// State conflict between transaction execution and current storage state.
    #[error("Storage conflict: {0:?}")]
    Conflict(ExecutionConflicts),
}
