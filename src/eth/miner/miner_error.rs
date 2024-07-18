use crate::eth::primitives::ExecutionConflicts;
use crate::eth::storage::StorageError;

#[derive(Debug, thiserror::Error, strum::EnumIs)]
pub enum MinerError {
    /// Conflict between transaction execution being saved and storage state.
    #[error("Execution conflict: {0:?}")]
    ExecutionConflict(Box<ExecutionConflicts>),

    /// Storage error while interacting with the miner.
    #[error("Unexpected storage error: {0}")]
    Storage(StorageError),

    /// Generic error interacting with the miner.
    #[error("Unexpected miner error: {0}")]
    Unexpected(anyhow::Error),
}

// -----------------------------------------------------------------------------
// Conversions: Other -> Self
// -----------------------------------------------------------------------------
impl From<anyhow::Error> for MinerError {
    fn from(value: anyhow::Error) -> Self {
        let value = match value.downcast::<StorageError>() {
            Ok(storage_error) => return Self::from(storage_error),
            Err(value) => value,
        };
        Self::Unexpected(value)
    }
}

impl From<StorageError> for MinerError {
    fn from(value: StorageError) -> Self {
        match value {
            StorageError::ExecutionConflict(conflicts) => Self::ExecutionConflict(conflicts),
            e => Self::Storage(e),
        }
    }
}
