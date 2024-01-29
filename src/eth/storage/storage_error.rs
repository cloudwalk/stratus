use crate::eth::primitives::TransactionExecutionConflicts;

#[derive(Debug, thiserror::Error, derive_new::new)]
pub enum EthStorageError {
    /// Generic error interacting with the storage.
    #[error("Storage error: {0}")]
    Generic(#[from] anyhow::Error),

    /// State conflict between transaction execution and current storage state.
    #[error("Storage conflict: {0:?}")]
    Conflict(TransactionExecutionConflicts),
}
